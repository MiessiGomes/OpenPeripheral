use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use duocast::peripherals::ajazz::aj179::{self, Mouse as Aj179, Snapshot as Aj179Snap};
use duocast::peripherals::hyperx_duocast::device::{enumerate, AudioHid, Lighting};
use duocast::peripherals::hyperx_duocast::effects::{frame, Effect};
use duocast::peripherals::hyperx_duocast::ids::{AUDIO_PID, CONTROLLER_PID};
use duocast::peripherals::hyperx_duocast::mixer::{self, MixerState};
use duocast::peripherals::hyperx_duocast::monitor::{Monitor, MonitorView};
use duocast::peripherals::hyperx_duocast::persist::{self, SavedState};
use duocast::peripherals::hyperx_duocast::protocol::PolarPattern;
use duocast::rgb::Rgb;
use duocast::update::{self, UpdateStatus};
use glib::Propagation;
use gtk::prelude::*;
use hidapi::HidApi;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt, WebView, WebViewExt};

const INDEX: &str = include_str!("../../web/index.html");
const CSS: &str = include_str!("../../web/app.css");
const JS: &str = include_str!("../../web/app.js");
const MIC3D: &str = include_str!("../../web/peripherals/hyperx_duocast/mic3d.js");
const LOGO: &str = include_str!("../../icons/gemini-logo.svg");

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
    aj179: Arc<Mutex<Aj179Snap>>,
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
    aj179: Aj179View,
    version: String,
    update: UpdateStatus,
}

#[derive(Serialize, Clone, Default)]
struct Aj179View {
    connected: bool,
    link: String,
    vid: String,
    pid: String,
    device_id: String,
    battery: Option<u8>,
    charging: bool,
    awake: bool,
    rate_hz: Option<u16>,
    dpi_index: Option<u8>,
    dpi: Vec<Aj179DpiView>,
    light_mode: Option<u8>,
    light_brightness: u8,
    light_speed: u8,
    light_color: String,
    lod: Option<u8>,
    motion_sync: bool,
    angle_snap: bool,
    sleep: Option<u8>,
    wake_on_move: bool,
    rx_fw: Option<String>,
    error: Option<String>,
    buttons: Vec<Aj179ButtonView>,
}

#[derive(Serialize, Clone)]
struct Aj179ButtonView {
    index: u8,
    name: String,
    action: String,
    locked: bool,
}

#[derive(Serialize, Clone)]
struct Aj179DpiView {
    value: u16,
    color: String,
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
struct Aj179Body {
    #[serde(default)]
    rate: Option<u16>,
    #[serde(default)]
    light_mode: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    brightness: Option<u8>,
    #[serde(default)]
    speed: Option<u8>,
    #[serde(default)]
    lod: Option<u8>,
    #[serde(default)]
    motion_sync: Option<bool>,
    #[serde(default)]
    angle_snap: Option<bool>,
    #[serde(default)]
    dpi_index: Option<u8>,
    #[serde(default)]
    dpi_values: Option<Vec<u16>>,
    #[serde(default)]
    dpi_colors: Option<Vec<String>>,
    #[serde(default)]
    button_index: Option<u8>,
    #[serde(default)]
    button_action: Option<String>,
    #[serde(default)]
    buttons_reset: bool,
}

#[derive(Deserialize, Default)]
struct MonitorBody {
    listen: Option<bool>,
    volume: Option<u8>,
    action: Option<String>,
}

fn main() {
    apply_display_workarounds();
    let addr = SocketAddr::from(([127, 0, 0, 1], 7840));
    let url = format!("http://{addr}");
    let already = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(80)).is_ok();
    if already {
        app_log("Já há um OpenPeripheral na porta 7840; a abrir janela contra esse servidor.");
    } else {
        thread::Builder::new()
            .name("openperipheral-http".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("tokio");
                rt.block_on(serve_http(addr));
            })
            .expect("servidor HTTP");
        wait_for_http(addr);
    }
    app_log(&format!("OpenPeripheral → {url}"));
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        app_log(&format!("Sem display gráfico; o painel fica só em {url}"));
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }
    open_native_window(&url);
}

fn apply_display_workarounds() {
    // NVIDIA + WebKitGTK: DMA-BUF deixa o WebView preto/cinza.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

fn app_log(msg: &str) {
    eprintln!("{msg}");
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dir = std::path::PathBuf::from(home).join(".config/openperipheral");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("app.log"))
    {
        use std::io::Write;
        let _ = writeln!(f, "{msg}");
    }
}

async fn serve_http(addr: SocketAddr) {
    let app = App::start();
    let router = Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/favicon.ico", get(|| async { logo() }))
        .route("/logo.svg", get(|| async { logo() }))
        .route("/app.css", get(|| async { css() }))
        .route("/app.js", get(|| async { js() }))
        .route("/mic3d.js", get(|| async { mic3d() }))
        .route("/api/state", get(state))
        .route("/api/meter", get(meter))
        .route("/api/led", post(led))
        .route("/api/audio", post(audio))
        .route("/api/monitor", post(monitor))
        .route("/api/aj179", post(aj179_api))
        .with_state(app);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            app_log(&format!("HTTP {addr}: {e}"));
            return;
        }
    };
    if let Err(e) = axum::serve(listener, router).await {
        app_log(&format!("servidor HTTP: {e}"));
    }
}

fn wait_for_http(addr: SocketAddr) {
    for _ in 0..80 {
        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(80)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("servidor HTTP não arrancou em {addr}");
}

fn open_native_window(url: &str) {
    let page = format!("{}/", url.trim_end_matches('/'));
    if let Some(mut child) = spawn_chrome(&page) {
        let pid = child.id();
        let _ = gtk::init();
        let show_item = MenuItem::with_id("show", "Mostrar", true, None);
        let quit_item = MenuItem::with_id("quit", "Sair", true, None);
        let menu = Menu::new();
        let _ = menu.append(&show_item);
        let _ = menu.append(&quit_item);
        let icon = load_tray_icon();
        let mut tray = TrayIconBuilder::new()
            .with_tooltip("OpenPeripheral")
            .with_menu(Box::new(menu));
        if let Some(icon) = icon {
            tray = tray.with_icon(icon);
        }
        let _tray = match tray.build() {
            Ok(t) => Some(t),
            Err(e) => {
                app_log(&format!("bandeja: {e}"));
                None
            }
        };
        thread::spawn(watch_minimize_to_tray);
        thread::spawn(move || {
            let _ = child.wait();
            app_log("janela fechada (X); a matar Chrome e o servidor");
            kill_everything(pid);
        });
        glib::timeout_add_local(Duration::from_millis(200), move || {
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                if matches!(
                    ev,
                    TrayIconEvent::Click { .. } | TrayIconEvent::DoubleClick { .. }
                ) {
                    show_app_window();
                }
            }
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                match ev.id.0.as_str() {
                    "show" => show_app_window(),
                    "quit" => kill_everything(pid),
                    _ => {}
                }
            }
            glib::ControlFlow::Continue
        });
        gtk::main();
        kill_everything(pid);
    }
    app_log("Sem Chrome/Chromium/Brave; a usar WebKitGTK.");
    open_gtk_webview(&page);
}

fn spawn_chrome(page: &str) -> Option<Child> {
    const BINS: &[&str] = &[
        "google-chrome-stable",
        "google-chrome",
        "brave-browser",
        "chromium",
        "chromium-browser",
        "microsoft-edge-stable",
        "microsoft-edge",
    ];
    let bin = BINS.iter().copied().find(|b| {
        Path::new(&format!("/usr/bin/{b}")).exists()
            || Path::new(&format!("/usr/local/bin/{b}")).exists()
    })?;
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let profile = Path::new(&home).join(".config/openperipheral/chrome-profile");
    let _ = std::fs::create_dir_all(&profile);
    app_log(&format!("Janela nativa via {bin} --app"));
    match Command::new(bin)
        .arg(format!("--app={page}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-features=TranslateUI")
        .arg("--disable-background-mode")
        .arg("--class=OpenPeripheral")
        .arg("--window-size=1280,840")
        .process_group(0)
        .spawn()
    {
        Ok(child) => Some(child),
        Err(e) => {
            app_log(&format!("falha a lançar {bin}: {e}"));
            None
        }
    }
}

fn load_tray_icon() -> Option<Icon> {
    let bytes = include_bytes!("../../icons/hicolor/64x64/apps/openperipheral.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}

fn kill_everything(chrome_pid: u32) -> ! {
    app_log("A terminar a aplicação (Chrome + servidor HTTP)");
    unsafe {
        libc::kill(chrome_pid as i32, libc::SIGTERM);
        libc::kill(-(chrome_pid as i32), libc::SIGTERM);
    }
    let _ = Command::new("pkill")
        .args(["-f", "openperipheral/chrome-profile"])
        .status();
    thread::sleep(Duration::from_millis(200));
    unsafe {
        libc::kill(chrome_pid as i32, libc::SIGKILL);
        libc::kill(-(chrome_pid as i32), libc::SIGKILL);
    }
    let _ = Command::new("pkill")
        .args(["-9", "-f", "openperipheral/chrome-profile"])
        .status();
    std::process::exit(0);
}

fn show_app_window() {
    for id in app_window_ids() {
        let _ = Command::new("xdotool").args(["windowmap", &id]).status();
        let _ = Command::new("wmctrl")
            .args(["-i", "-r", &id, "-b", "remove,hidden"])
            .status();
        let _ = Command::new("xdotool")
            .args(["windowactivate", &id])
            .status();
        let _ = Command::new("xdotool").args(["windowraise", &id]).status();
        let _ = Command::new("wmctrl").args(["-i", "-a", &id]).status();
    }
}

fn app_window_ids() -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(out) = Command::new("xdotool")
        .args(["search", "--name", "OpenPeripheral"])
        .output()
    {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let id = line.trim();
                if !id.is_empty() {
                    ids.push(id.into());
                }
            }
        }
    }
    if ids.is_empty() {
        if let Ok(out) = Command::new("wmctrl").args(["-l"]).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if line.contains("OpenPeripheral") {
                    if let Some(id) = line.split_whitespace().next() {
                        ids.push(id.into());
                    }
                }
            }
        }
    }
    ids
}

fn window_is_iconic(id: &str) -> bool {
    let Ok(out) = Command::new("xprop").args(["-id", id, "WM_STATE"]).output() else {
        return false;
    };
    let s = String::from_utf8_lossy(&out.stdout);
    s.contains("Iconic")
}

fn window_is_viewable(id: &str) -> bool {
    let Ok(out) = Command::new("xwininfo").args(["-id", id]).output() else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout).contains("IsViewable")
}

fn watch_minimize_to_tray() {
    loop {
        for id in app_window_ids() {
            if window_is_viewable(&id) && window_is_iconic(&id) {
                let _ = Command::new("xdotool").args(["windowunmap", &id]).status();
                app_log("minimizado para a bandeja");
            }
        }
        thread::sleep(Duration::from_millis(400));
    }
}

fn open_gtk_webview(page: &str) {
    gtk::init().expect("gtk");
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("OpenPeripheral");
    window.set_default_size(1280, 840);
    window.set_position(gtk::WindowPosition::Center);

    let webview = WebView::new();
    webview.set_hexpand(true);
    webview.set_vexpand(true);
    if let Some(settings) = WebViewExt::settings(&webview) {
        settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
        settings.set_enable_developer_extras(cfg!(debug_assertions));
        settings.set_javascript_can_open_windows_automatically(false);
    }
    webview.connect_load_failed(|_, _, uri, err| {
        app_log(&format!("WebKit falhou {uri}: {err}"));
        false
    });
    webview.connect_load_changed(move |_, event| {
        app_log(&format!("WebKit load {event:?}"));
    });
    webview.load_uri(page);
    window.add(&webview);

    window.connect_window_state_event(|w, ev| {
        let iconic = gtk::gdk::WindowState::ICONIFIED;
        if ev.changed_mask().contains(iconic) && ev.new_window_state().contains(iconic) {
            w.hide();
            app_log("minimizado para a bandeja");
        }
        Propagation::Proceed
    });
    window.connect_delete_event(move |_, _| {
        app_log("janela fechada; a terminar o processo");
        webview.try_close();
        gtk::main_quit();
        Propagation::Proceed
    });
    window.connect_destroy(|_| {
        std::process::exit(0);
    });
    window.show_all();
    gtk::main();
    std::process::exit(0);
}

fn css() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS).into_response()
}

fn logo() -> Response {
    (
        [(header::CONTENT_TYPE, "image/svg+xml; charset=utf-8")],
        LOGO,
    )
        .into_response()
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
        let aj179 = Arc::new(Mutex::new(Aj179Snap::default()));
        spawn_aj179_poller(aj179.clone());
        let update = Arc::new(Mutex::new(UpdateStatus::default()));
        spawn_update_checker(update.clone());
        Self {
            live,
            status,
            lighting_ok,
            audio_ok,
            hid,
            aj179,
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

async fn aj179_api(State(app): State<App>, Json(body): Json<Aj179Body>) -> Response {
    match apply_aj179(&app, body) {
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
    let aj179 = aj179_view(&app.aj179.lock().unwrap());
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
        aj179,
        version: update::current_version().into(),
        update: app.update.lock().unwrap().clone(),
    }
}

fn aj179_view(s: &Aj179Snap) -> Aj179View {
    let info = s.info.as_ref();
    Aj179View {
        connected: s.connected,
        link: s.link.clone(),
        vid: format!("{:04x}", s.vid),
        pid: format!("{:04x}", s.pid),
        device_id: info.map(|i| i.device_id.clone()).unwrap_or_default(),
        battery: info.map(|i| i.battery),
        charging: info.map(|i| i.charging).unwrap_or(false),
        awake: info.map(|i| i.connected).unwrap_or(false),
        rate_hz: s.rate_hz,
        dpi_index: s.dpi_index,
        dpi: s
            .dpi
            .iter()
            .map(|d| Aj179DpiView {
                value: d.value,
                color: d.color.to_hex(),
            })
            .collect(),
        light_mode: s.lighting.as_ref().map(|l| l.mode),
        light_brightness: s.lighting.as_ref().map(|l| l.brightness).unwrap_or(4),
        light_speed: s.lighting.as_ref().map(|l| l.speed).unwrap_or(2),
        light_color: s
            .lighting
            .as_ref()
            .map(|l| l.color.to_hex())
            .unwrap_or_else(|| "#ff0000".into()),
        lod: s.sensor.as_ref().map(|x| x.lod),
        motion_sync: s.sensor.as_ref().map(|x| x.motion_sync).unwrap_or(false),
        angle_snap: s.sensor.as_ref().map(|x| x.angle_snap).unwrap_or(false),
        sleep: s.power.as_ref().map(|x| x.sleep),
        wake_on_move: s.power.as_ref().map(|x| x.wake_on_move).unwrap_or(false),
        rx_fw: s.rx_fw.clone(),
        error: s.error.clone(),
        buttons: s
            .buttons
            .iter()
            .enumerate()
            .map(|(i, code)| Aj179ButtonView {
                index: i as u8,
                name: aj179::BUTTON_NAMES.get(i).copied().unwrap_or("?").into(),
                action: aj179::button_action(*code),
                locked: i == 0,
            })
            .collect(),
    }
}

fn apply_aj179(app: &App, body: Aj179Body) -> Result<(), String> {
    let mouse = Aj179::open().map_err(|e| e.to_string())?;
    if let Some(hz) = body.rate {
        mouse.set_rate(hz).map_err(|e| e.to_string())?;
        *app.status.lock().unwrap() = format!("AJ179 taxa {hz} Hz");
    }
    if let Some(mode) = body.light_mode.as_deref() {
        let m = match mode {
            "off" => aj179::LIGHT_OFF,
            "breathing" => aj179::LIGHT_BREATHING,
            "constant" => aj179::LIGHT_CONSTANT,
            _ => return Err(format!("modo de luz desconhecido: {mode}")),
        };
        let color = body
            .color
            .as_deref()
            .map(Rgb::parse_hex)
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(Rgb::new(0xff, 0, 0));
        mouse
            .set_lighting(
                m,
                body.brightness.unwrap_or(4),
                body.speed.unwrap_or(2),
                color,
            )
            .map_err(|e| e.to_string())?;
        *app.status.lock().unwrap() = "AJ179 iluminação atualizada".into();
    }
    if body.lod.is_some() || body.motion_sync.is_some() || body.angle_snap.is_some() {
        let cur = mouse.snapshot();
        let lod = body
            .lod
            .or_else(|| cur.sensor.as_ref().map(|s| s.lod))
            .unwrap_or(2);
        let ms = body
            .motion_sync
            .or_else(|| cur.sensor.as_ref().map(|s| s.motion_sync))
            .unwrap_or(false);
        let ang = body
            .angle_snap
            .or_else(|| cur.sensor.as_ref().map(|s| s.angle_snap))
            .unwrap_or(false);
        mouse.set_sensor(lod, ms, ang).map_err(|e| e.to_string())?;
        *app.status.lock().unwrap() = "AJ179 sensor atualizado".into();
    }
    if body.dpi_index.is_some() || body.dpi_values.is_some() || body.dpi_colors.is_some() {
        let cur = mouse.snapshot();
        let sensor = cur
            .info
            .as_ref()
            .map(|i| i.sensor_id)
            .unwrap_or(aj179::SENSOR_PAW3395);
        let mut stages: Vec<u16> = cur.dpi.iter().map(|d| d.value).collect();
        if let Some(vals) = &body.dpi_values {
            if vals.len() < 2 {
                return Err("DPI precisa de pelo menos 2 estágios".into());
            }
            stages = vals.iter().map(|v| aj179::clamp_dpi(sensor, *v)).collect();
        }
        if stages.is_empty() {
            return Err("DPI ainda não lido; mexa no mouse".into());
        }
        let idx = body.dpi_index.unwrap_or(cur.dpi_index.unwrap_or(0));
        let idx = idx.min(stages.len().saturating_sub(1) as u8);
        mouse
            .set_dpi_stages(sensor, idx, &stages)
            .map_err(|e| e.to_string())?;
        if let Some(cols) = &body.dpi_colors {
            let mut colors: Vec<Rgb> = cur.dpi.iter().map(|d| d.color).collect();
            colors.resize(stages.len(), Rgb::OFF);
            for (i, hex) in cols.iter().enumerate() {
                if i < colors.len() {
                    colors[i] = Rgb::parse_hex(hex).map_err(|e| e.to_string())?;
                }
            }
            mouse.set_dpi_colors(&colors).map_err(|e| e.to_string())?;
        }
        *app.status.lock().unwrap() = format!("AJ179 DPI estágio {}", idx + 1);
    }
    if body.buttons_reset {
        mouse
            .set_buttons(&aj179::FACTORY_BUTTONS)
            .map_err(|e| e.to_string())?;
        *app.status.lock().unwrap() = "AJ179 botões de fábrica".into();
    } else if let (Some(idx), Some(action)) = (body.button_index, body.button_action.as_deref()) {
        if idx == 0 {
            return Err("o botão esquerdo não é remapeado (fica o clique)".into());
        }
        if idx as usize >= aj179::BUTTON_COUNT {
            return Err("índice de botão inválido".into());
        }
        let cur = mouse.snapshot();
        let mut keys = cur.buttons;
        if keys.len() < aj179::BUTTON_COUNT {
            keys = aj179::FACTORY_BUTTONS.to_vec();
        }
        keys[idx as usize] = aj179::encode_button(action).map_err(|e| e.to_string())?;
        mouse.set_buttons(&keys).map_err(|e| e.to_string())?;
        *app.status.lock().unwrap() = format!(
            "AJ179 botão {} → {action}",
            aj179::BUTTON_NAMES[idx as usize]
        );
    }
    *app.aj179.lock().unwrap() = mouse.snapshot();
    Ok(())
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
        Ok(api) => {
            let duo = enumerate(&api).into_iter().map(|d| {
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
            });
            let mice = aj179::enumerate(&api).into_iter().map(|d| {
                let role = if d.interface == aj179::CONFIG_INTERFACE {
                    "AJ179 config HID"
                } else {
                    "AJ179"
                };
                DeviceView {
                    vid: format!("{:04x}", d.vid),
                    pid: format!("{:04x}", d.pid),
                    interface: d.interface,
                    product: if d.product.is_empty() {
                        "AJAZZ AJ179".into()
                    } else {
                        d.product
                    },
                    path: d.path,
                    role: role.into(),
                }
            });
            duo.chain(mice).collect()
        }
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
    let duo = PeripheralView {
        id: "duocast".into(),
        brand: "HyperX".into(),
        name: "DuoCast".into(),
        kind: "microphone".into(),
        connected,
        summary,
        details,
        pages: vec!["light".into(), "audio".into()],
    };
    let aj_parts: Vec<&DeviceView> = devices
        .iter()
        .filter(|d| {
            aj179::is_aj179_ids(
                u16::from_str_radix(&d.vid, 16).unwrap_or(0),
                u16::from_str_radix(&d.pid, 16).unwrap_or(0),
            )
        })
        .collect();
    let aj_on = !aj_parts.is_empty();
    let aj_details = aj_parts
        .iter()
        .map(|d| format!("{}:{}  iface {}  {}", d.vid, d.pid, d.interface, d.role))
        .collect();
    let aj = PeripheralView {
        id: "aj179".into(),
        brand: "AJAZZ".into(),
        name: "AJ179".into(),
        kind: "mouse".into(),
        connected: aj_on,
        summary: if aj_on {
            format!("{} interfaces USB", aj_parts.len())
        } else {
            "desconectado".into()
        },
        details: aj_details,
        pages: vec!["light".into(), "mouse".into()],
    };
    vec![duo, aj]
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
    let duo = std::path::Path::new("/usr/lib/udev/rules.d/99-hyperx-duocast.rules").exists()
        || std::path::Path::new("/etc/udev/rules.d/99-hyperx-duocast.rules").exists();
    let mouse = std::path::Path::new("/usr/lib/udev/rules.d/99-ajazz-aj179.rules").exists()
        || std::path::Path::new("/etc/udev/rules.d/99-ajazz-aj179.rules").exists();
    duo || mouse
}

fn spawn_aj179_poller(slot: Arc<Mutex<Aj179Snap>>) {
    thread::Builder::new()
        .name("aj179-hid".into())
        .spawn(move || loop {
            match Aj179::open() {
                Ok(m) => *slot.lock().unwrap() = m.snapshot(),
                Err(e) => {
                    *slot.lock().unwrap() = Aj179Snap {
                        error: Some(e.to_string()),
                        ..Aj179Snap::default()
                    };
                }
            }
            thread::sleep(Duration::from_millis(2000));
        })
        .ok();
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
