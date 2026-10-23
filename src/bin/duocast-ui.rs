use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use duocast::device::{enumerate, AudioHid, Lighting};
use duocast::effects::{frame, Effect};
use duocast::ids::{AUDIO_PID, CONTROLLER_PID};
use duocast::mixer::{self, MixerState};
use duocast::monitor::{Monitor, MonitorView};
use duocast::persist::{self, SavedState};
use duocast::protocol::{PolarPattern, Rgb};
use duocast::update::{self, UpdateStatus};
use hidapi::HidApi;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const INDEX: &str = include_str!("../../web/index.html");
const CSS: &str = include_str!("../../web/app.css");
const JS: &str = include_str!("../../web/app.js");
const MIC3D: &str = include_str!("../../web/mic3d.js");

#[derive(Clone, Copy, Debug)]
enum LiveLed {
    /// Não envia HID: mantém a cor já gravada no firmware.
    Idle,
    Solid {
        top: Rgb,
        bottom: Rgb,
    },
    Effect {
        kind: Effect,
        color: Rgb,
        brightness: u8,
        speed: u16,
    },
    Save {
        top: Rgb,
        bottom: Rgb,
    },
    FirmwareReset,
}

#[derive(Clone)]
struct App {
    live: Arc<Mutex<LiveLed>>,
    status: Arc<Mutex<String>>,
    lighting_ok: Arc<AtomicBool>,
    audio_ok: Arc<AtomicBool>,
    hid: Arc<Mutex<HidSnapshot>>,
    prefs: Arc<Mutex<LedPrefs>>,
    monitor: Arc<Monitor>,
    persist_fw: Arc<AtomicBool>,
    led_touch: Arc<Mutex<Instant>>,
    fx_speed: Arc<Mutex<u16>>,
    update: Arc<Mutex<UpdateStatus>>,
}

#[derive(Clone, Copy)]
struct LedPrefs {
    top: Rgb,
    bottom: Rgb,
    brightness: u8,
    link: bool,
}

#[derive(Clone, Default, Serialize)]
struct HidSnapshot {
    polar: Option<PolarPattern>,
    mute: Option<bool>,
    highpass: Option<u8>,
    gain: Option<u8>,
    sidetone_mute: Option<bool>,
    ping: Option<String>,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
struct DeviceView {
    vid: String,
    pid: String,
    interface: i32,
    product: String,
    path: String,
    role: String,
}

#[derive(Serialize)]
struct AppStateView {
    lighting_ok: bool,
    audio_ok: bool,
    notice: String,
    permission_hint: bool,
    udev_installed: bool,
    devices: Vec<DeviceView>,
    mixer: MixerState,
    mixer_error: Option<String>,
    hid: HidSnapshot,
    led: LedView,
    monitor: MonitorView,
    muted: bool,
    peripherals: Vec<PeripheralView>,
    version: String,
    update: UpdateStatus,
}

#[derive(Serialize)]
struct PeripheralView {
    id: String,
    brand: String,
    name: String,
    kind: String,
    connected: bool,
    summary: String,
    details: Vec<String>,
    pages: Vec<String>,
}

#[derive(Serialize)]
struct LedView {
    top: String,
    bottom: String,
    brightness: u8,
    link: bool,
    effect: Option<String>,
    speed: u16,
}

#[derive(Deserialize)]
struct LedBody {
    mode: String,
    #[serde(default)]
    top: Option<String>,
    #[serde(default)]
    bottom: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    brightness: Option<u8>,
    #[serde(default)]
    link: Option<bool>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    save: bool,
    #[serde(default)]
    speed: Option<u16>,
}

#[derive(Deserialize, Default)]
struct AudioBody {
    polar: Option<PolarPattern>,
    hid_mute: Option<bool>,
    highpass: Option<u8>,
    gain: Option<u8>,
    sidetone_mute: Option<bool>,
    mic_capture: Option<bool>,
    pcm_percent: Option<u8>,
    pcm_on: Option<bool>,
    extension: Option<bool>,
    ping: Option<bool>,
}

#[derive(Deserialize, Default)]
struct MonitorBody {
    listen: Option<bool>,
    volume: Option<u8>,
    action: Option<String>,
}

#[tokio::main]
async fn main() {
    let app = App::start();
    let router = Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .route("/app.css", get(|| async { css() }))
        .route("/app.js", get(|| async { js() }))
        .route("/mic3d.js", get(|| async { mic3d() }))
        .route("/api/state", get(state))
        .route("/api/meter", get(meter))
        .route("/api/led", post(led))
        .route("/api/audio", post(audio))
        .route("/api/monitor", post(monitor))
        .with_state(app);

    let addr = SocketAddr::from(([127, 0, 0, 1], 7840));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("porta 7840");
    let url = format!("http://{addr}");
    eprintln!("OpenPeripheral → {url}");
    let _ = Command::new("xdg-open").arg(&url).spawn();
    axum::serve(listener, router).await.expect("servidor");
}

fn css() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS).into_response()
}

fn js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        JS,
    )
        .into_response()
}

fn mic3d() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        MIC3D,
    )
        .into_response()
}

impl App {
    fn start() -> Self {
        let saved = persist::load();
        let top = persist::parse_rgb(&saved.top, Rgb::new(0xf2, 0, 0));
        let bottom = persist::parse_rgb(&saved.bottom, top);
        let brightness = saved.brightness.clamp(1, 100);
        let prefs = LedPrefs {
            top,
            bottom,
            brightness,
            link: saved.link,
        };
        let live = Arc::new(Mutex::new(LiveLed::Idle));
        let status = Arc::new(Mutex::new(String::new()));
        let lighting_ok = Arc::new(AtomicBool::new(false));
        let audio_ok = Arc::new(AtomicBool::new(false));
        let hid = Arc::new(Mutex::new(HidSnapshot::default()));
        let persist_fw = Arc::new(AtomicBool::new(false));
        let led_touch = Arc::new(Mutex::new(Instant::now()));
        spawn_led_worker(
            live.clone(),
            status.clone(),
            lighting_ok.clone(),
            persist_fw.clone(),
            led_touch.clone(),
        );
        spawn_hid_poller(hid.clone(), audio_ok.clone());
        let update = Arc::new(Mutex::new(UpdateStatus::default()));
        spawn_update_checker(update.clone());
        Self {
            live,
            status,
            lighting_ok,
            audio_ok,
            hid,
            prefs: Arc::new(Mutex::new(prefs)),
            monitor: Arc::new(Monitor::new()),
            persist_fw,
            led_touch,
            fx_speed: Arc::new(Mutex::new(100)),
            update,
        }
    }
}

async fn state(State(app): State<App>) -> Json<AppStateView> {
    Json(collect_state(&app))
}

#[derive(Serialize)]
struct MeterView {
    #[serde(flatten)]
    monitor: MonitorView,
    muted: bool,
}

async fn meter(State(app): State<App>) -> Json<MeterView> {
    app.monitor.decay_meters();
    let mixer = mixer::snapshot().unwrap_or_default();
    let hid = app.hid.lock().unwrap().clone();
    Json(MeterView {
        monitor: app.monitor.snapshot(),
        muted: is_muted(&mixer, &hid),
    })
}

fn is_muted(mixer: &MixerState, hid: &HidSnapshot) -> bool {
    !mixer.mic_capture || hid.mute == Some(true)
}

async fn led(State(app): State<App>, Json(body): Json<LedBody>) -> Response {
    match apply_led(&app, body) {
        Ok(()) => Json(collect_state(&app)).into_response(),
        Err(e) => err_json(&e),
    }
}

async fn audio(State(app): State<App>, Json(body): Json<AudioBody>) -> Response {
    match apply_audio(&app, body) {
        Ok(()) => Json(collect_state(&app)).into_response(),
        Err(e) => err_json(&e),
    }
}

async fn monitor(State(app): State<App>, Json(body): Json<MonitorBody>) -> Response {
    match apply_monitor(&app, body) {
        Ok(()) => Json(collect_state(&app)).into_response(),
        Err(e) => err_json(&e),
    }
}

fn err_json(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

fn collect_state(app: &App) -> AppStateView {
    let notice = app.status.lock().unwrap().clone();
    let prefs = *app.prefs.lock().unwrap();
    let live = *app.live.lock().unwrap();
    let effect = match live {
        LiveLed::Effect { kind, .. } => Some(kind.label().to_lowercase()),
        _ => None,
    };
    let speed = *app.fx_speed.lock().unwrap();
    let (devices, audio_listed) = list_devices();
    let permission_hint = !app.lighting_ok.load(Ordering::Relaxed)
        && devices
            .iter()
            .any(|d| d.pid == format!("{:04x}", CONTROLLER_PID));
    let (mixer, mixer_error) = match mixer::snapshot() {
        Ok(s) => (s, None),
        Err(e) => (MixerState::default(), Some(e.to_string())),
    };
    let hid = app.hid.lock().unwrap().clone();
    let muted = is_muted(&mixer, &hid);
    let peripherals = group_peripherals(&devices);
    AppStateView {
        lighting_ok: app.lighting_ok.load(Ordering::Relaxed),
        audio_ok: app.audio_ok.load(Ordering::Relaxed) || audio_listed,
        notice,
        permission_hint,
        udev_installed: udev_rules_installed(),
        devices,
        mixer,
        mixer_error,
        hid,
        led: LedView {
            top: prefs.top.to_hex(),
            bottom: prefs.bottom.to_hex(),
            brightness: prefs.brightness,
            link: prefs.link,
            effect,
            speed,
        },
        monitor: {
            app.monitor.decay_meters();
            app.monitor.snapshot()
        },
        muted,
        peripherals,
        version: update::current_version().into(),
        update: app.update.lock().unwrap().clone(),
    }
}

fn apply_monitor(app: &App, body: MonitorBody) -> Result<(), String> {
    if let Some(v) = body.volume {
        app.monitor.set_volume(v);
        if body.listen.is_none() && body.action.is_none() {
            *app.status.lock().unwrap() = format!("Volume do teste: {v}%");
        }
    }
    if let Some(on) = body.listen {
        if on {
            let _ = mixer::set_mute(false);
        }
        app.monitor.set_listening(on).map_err(|e| e.to_string())?;
        *app.status.lock().unwrap() = if on {
            "A ouvir o DuoCast na saída do PC.".into()
        } else {
            "Monitor do PC desligado.".into()
        };
    }
    match body.action.as_deref() {
        Some("record") => {
            let _ = mixer::set_mute(false);
            app.monitor.record_and_play().map_err(|e| e.to_string())?;
            *app.status.lock().unwrap() = "Gravando 4s e em seguida reproduzindo.".into();
        }
        Some("play") => {
            app.monitor.play_take().map_err(|e| e.to_string())?;
            *app.status.lock().unwrap() = "A reproduzir o último trecho.".into();
        }
        Some(other) => return Err(format!("ação de monitor desconhecida: {other}")),
        None => {}
    }
    Ok(())
}

fn list_devices() -> (Vec<DeviceView>, bool) {
    let mut audio_listed = false;
    let devices = match HidApi::new() {
        Ok(api) => enumerate(&api)
            .into_iter()
            .map(|d| {
                if d.pid == AUDIO_PID {
                    audio_listed = true;
                }
                let role = match (d.pid, d.interface) {
                    (CONTROLLER_PID, 0) => "LED (feature HID)",
                    (CONTROLLER_PID, _) => "controlador HID",
                    (AUDIO_PID, 3) => "áudio HID 0x66",
                    (AUDIO_PID, _) => "áudio USB",
                    _ => "DuoCast",
                };
                DeviceView {
                    vid: format!("{:04x}", d.vid),
                    pid: format!("{:04x}", d.pid),
                    interface: d.interface,
                    product: if d.product.is_empty() {
                        "HyperX DuoCast".into()
                    } else {
                        d.product
                    },
                    path: d.path,
                    role: role.into(),
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    (devices, audio_listed)
}

fn group_peripherals(devices: &[DeviceView]) -> Vec<PeripheralView> {
    let parts: Vec<&DeviceView> = devices
        .iter()
        .filter(|d| {
            d.pid == format!("{:04x}", CONTROLLER_PID) || d.pid == format!("{:04x}", AUDIO_PID)
        })
        .collect();
    let connected = !parts.is_empty();
    let details = parts
        .iter()
        .map(|d| format!("{}:{}  iface {}  {}", d.vid, d.pid, d.interface, d.role))
        .collect();
    let summary = if connected {
        format!("{} interfaces USB", parts.len())
    } else {
        "desconectado".into()
    };
    vec![PeripheralView {
        id: "duocast".into(),
        brand: "HyperX".into(),
        name: "DuoCast".into(),
        kind: "microphone".into(),
        connected,
        summary,
        details,
        pages: vec!["light".into(), "audio".into()],
    }]
}

fn apply_led(app: &App, body: LedBody) -> Result<(), String> {
    let mut prefs = app.prefs.lock().unwrap();
    if let Some(b) = body.brightness {
        prefs.brightness = b.clamp(1, 100);
    }
    if let Some(link) = body.link {
        prefs.link = link;
    }
    if let Some(t) = body.top.as_deref().or(body.color.as_deref()) {
        prefs.top = Rgb::parse_hex(t)?;
        if prefs.link {
            prefs.bottom = prefs.top;
        }
    }
    if let Some(b) = body.bottom.as_deref() {
        prefs.bottom = Rgb::parse_hex(b)?;
    }
    persist::save(&SavedState {
        top: prefs.top.to_hex(),
        bottom: prefs.bottom.to_hex(),
        brightness: prefs.brightness,
        link: prefs.link,
    });
    let top = prefs.top.scaled(prefs.brightness);
    let bottom = prefs.bottom.scaled(prefs.brightness);
    let color = prefs.top;
    let brightness = prefs.brightness;
    drop(prefs);

    match body.mode.as_str() {
        "solid" => {
            *app.led_touch.lock().unwrap() = Instant::now();
            app.persist_fw.store(true, Ordering::Relaxed);
            if body.save {
                *app.live.lock().unwrap() = LiveLed::Save { top, bottom };
                *app.status.lock().unwrap() = "A gravar no controlador…".into();
            } else {
                *app.live.lock().unwrap() = LiveLed::Solid { top, bottom };
                *app.status.lock().unwrap() = "LED atualizado.".into();
            }
        }
        "off" => {
            {
                let mut p = app.prefs.lock().unwrap();
                p.top = Rgb::OFF;
                p.bottom = Rgb::OFF;
            }
            persist::save(&SavedState {
                top: "#000000".into(),
                bottom: "#000000".into(),
                brightness,
                link: true,
            });
            *app.led_touch.lock().unwrap() = Instant::now();
            app.persist_fw.store(true, Ordering::Relaxed);
            *app.live.lock().unwrap() = LiveLed::Solid {
                top: Rgb::OFF,
                bottom: Rgb::OFF,
            };
            *app.status.lock().unwrap() = "LED apagado.".into();
        }
        "reset" => {
            *app.live.lock().unwrap() = LiveLed::FirmwareReset;
            *app.status.lock().unwrap() = "Reset de LED no firmware…".into();
        }
        "effect" => {
            let kind = match body.kind.as_deref().and_then(Effect::from_name) {
                Some(k) => k,
                None => match *app.live.lock().unwrap() {
                    LiveLed::Effect { kind, .. } => kind,
                    _ => return Err("efeito desconhecido".into()),
                },
            };
            let speed: u16 = body
                .speed
                .unwrap_or_else(|| *app.fx_speed.lock().unwrap())
                .clamp(20, 400);
            *app.fx_speed.lock().unwrap() = speed;
            *app.live.lock().unwrap() = LiveLed::Effect {
                kind,
                color,
                brightness,
                speed,
            };
            *app.status.lock().unwrap() = format!(
                "Efeito {} no host · {:.1}×.",
                kind.label(),
                speed as f32 / 100.0
            );
        }
        other => return Err(format!("modo LED desconhecido: {other}")),
    }
    Ok(())
}

fn apply_audio(app: &App, body: AudioBody) -> Result<(), String> {
    if let Some(cap) = body.mic_capture {
        mixer::set_mute(!cap).map_err(|e| e.to_string())?;
    }
    if let Some(pct) = body.pcm_percent {
        let on = body.pcm_on.unwrap_or(true);
        mixer::set_monitor(pct, on).map_err(|e| e.to_string())?;
    } else if let Some(on) = body.pcm_on {
        let pct = mixer::snapshot().map(|s| s.pcm_percent).unwrap_or(40);
        mixer::set_monitor(pct, on).map_err(|e| e.to_string())?;
    }
    if let Some(ext) = body.extension {
        mixer::set_extension(ext).map_err(|e| e.to_string())?;
    }
    if let Some(v) = body.gain {
        app.monitor.set_mic_gain(v);
        app.hid.lock().unwrap().gain = Some(v.min(100));
    }
    if let Some(v) = body.highpass {
        app.monitor.set_highpass(v);
        app.hid.lock().unwrap().highpass = Some(v.min(100));
    }

    let needs_hid = body.polar.is_some()
        || body.hid_mute.is_some()
        || body.highpass.is_some()
        || body.gain.is_some()
        || body.sidetone_mute.is_some()
        || body.ping.unwrap_or(false);
    if needs_hid {
        with_audio(|a| {
            if let Some(p) = body.polar {
                a.set_polar(p)?;
            }
            if let Some(v) = body.hid_mute {
                a.set_muted(v)?;
            }
            if let Some(v) = body.highpass {
                app.monitor.set_highpass(v);
                a.set_highpass(v)?;
            }
            if let Some(v) = body.gain {
                app.monitor.set_mic_gain(v);
                let idx = ((u16::from(v.min(100)) * 20) / 100) as u8;
                a.set_gain_index(idx)?;
            }
            if let Some(v) = body.sidetone_mute {
                a.set_sidetone_muted(v)?;
            }
            let ping = if body.ping.unwrap_or(false) {
                let r = a.ping()?;
                Some(
                    r.iter()
                        .take(6)
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            } else {
                None
            };
            let mut snap = read_hid(a, ping);
            if let Some(v) = body.gain {
                snap.gain = Some(v);
            }
            if let Some(v) = body.highpass {
                snap.highpass = Some(v);
            }
            *app.hid.lock().unwrap() = snap;
            app.audio_ok.store(true, Ordering::Relaxed);
            Ok(())
        })?;
    }
    *app.status.lock().unwrap() = "Áudio atualizado.".into();
    Ok(())
}

fn with_audio<T>(f: impl FnOnce(&AudioHid) -> duocast::Result<T>) -> Result<T, String> {
    let audio = AudioHid::open().map_err(|e| e.to_string())?;
    f(&audio).map_err(|e| e.to_string())
}

fn read_hid(a: &AudioHid, ping: Option<String>) -> HidSnapshot {
    HidSnapshot {
        polar: a.polar().ok(),
        mute: a.muted().ok(),
        highpass: a.highpass().ok().filter(|v| *v <= 100),
        gain: a.gain_index().ok().filter(|v| *v <= 100),
        sidetone_mute: a.sidetone_muted().ok(),
        ping,
        error: None,
    }
}

fn spawn_led_worker(
    live: Arc<Mutex<LiveLed>>,
    status: Arc<Mutex<String>>,
    lighting_ok: Arc<AtomicBool>,
    persist_fw: Arc<AtomicBool>,
    led_touch: Arc<Mutex<Instant>>,
) {
    thread::Builder::new()
        .name("duocast-led".into())
        .spawn(move || loop {
            let api = match HidApi::new() {
                Ok(a) => a,
                Err(e) => {
                    lighting_ok.store(false, Ordering::Relaxed);
                    *status.lock().unwrap() = format!("hidapi: {e}");
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            let led = match Lighting::open(&api) {
                Ok(l) => l,
                Err(e) => {
                    lighting_ok.store(false, Ordering::Relaxed);
                    *status.lock().unwrap() = e.to_string();
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            lighting_ok.store(true, Ordering::Relaxed);
            *status.lock().unwrap() = "Controlador LED conectado.".into();
            let start = Instant::now();
            loop {
                let cmd = *live.lock().unwrap();
                let result = match cmd {
                    LiveLed::Idle => Ok(String::new()),
                    LiveLed::Save { top, bottom } => {
                        let r = led.save_solid(top, bottom);
                        *live.lock().unwrap() = LiveLed::Solid { top, bottom };
                        r.map(|_| "Gravado no controlador.".into())
                    }
                    LiveLed::FirmwareReset => {
                        let r = led.firmware_reset();
                        *live.lock().unwrap() = LiveLed::Solid {
                            top: Rgb::OFF,
                            bottom: Rgb::OFF,
                        };
                        r.map(|_| "Reset LED enviado.".into())
                    }
                    LiveLed::Solid { top, bottom } => {
                        let idle =
                            led_touch.lock().unwrap().elapsed() >= Duration::from_millis(800);
                        if persist_fw.load(Ordering::Relaxed) && idle {
                            persist_fw.store(false, Ordering::Relaxed);
                            led.save_solid(top, bottom)
                                .map(|_| "Cor salva no microfone.".into())
                        } else {
                            led.set_direct(top, bottom).map(|_| String::new())
                        }
                    }
                    LiveLed::Effect {
                        kind,
                        color,
                        brightness,
                        speed,
                    } => {
                        let (top, bottom) = frame(
                            kind,
                            start.elapsed().as_secs_f32(),
                            color,
                            brightness,
                            speed as f32 / 100.0,
                        );
                        led.set_direct(top, bottom).map(|_| String::new())
                    }
                };
                match result {
                    Ok(msg) if !msg.is_empty() => *status.lock().unwrap() = msg,
                    Ok(_) => {}
                    Err(e) => {
                        lighting_ok.store(false, Ordering::Relaxed);
                        *status.lock().unwrap() = e.to_string();
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(40));
            }
        })
        .ok();
}

fn udev_rules_installed() -> bool {
    std::path::Path::new("/usr/lib/udev/rules.d/99-hyperx-duocast.rules").exists()
        || std::path::Path::new("/etc/udev/rules.d/99-hyperx-duocast.rules").exists()
}

fn spawn_update_checker(slot: Arc<Mutex<UpdateStatus>>) {
    thread::Builder::new()
        .name("openperipheral-update".into())
        .spawn(move || {
            let status = update::check();
            *slot.lock().unwrap() = status;
        })
        .ok();
}

fn spawn_hid_poller(hid: Arc<Mutex<HidSnapshot>>, audio_ok: Arc<AtomicBool>) {
    thread::Builder::new()
        .name("duocast-hid".into())
        .spawn(move || loop {
            match AudioHid::open() {
                Ok(a) => {
                    let prev = hid.lock().unwrap().clone();
                    let mut snap = read_hid(&a, None);
                    snap.gain = prev.gain.or(snap.gain);
                    snap.highpass = prev.highpass.or(snap.highpass);
                    audio_ok.store(true, Ordering::Relaxed);
                    *hid.lock().unwrap() = snap;
                }
                Err(e) => {
                    hid.lock().unwrap().error = Some(e.to_string());
                }
            }
            thread::sleep(Duration::from_secs(2));
        })
        .ok();
}
