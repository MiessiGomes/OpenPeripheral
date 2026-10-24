use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use duocast::peripherals::ajazz::aj179::{self, Mouse as Aj179};
use duocast::peripherals::ajazz::persist as aj179_led;
use duocast::peripherals::hyperx_duocast::device::{enumerate, AudioHid, Lighting};
use duocast::peripherals::hyperx_duocast::effects::{frame, Effect};
use duocast::peripherals::hyperx_duocast::mixer;
use duocast::peripherals::hyperx_duocast::protocol::PolarPattern;
use duocast::rgb::Rgb;
use hidapi::HidApi;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    name = "duocast",
    about = "Controle Linux OpenPeripheral (DuoCast + AJ179)",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Lista interfaces USB DuoCast visíveis pelo hidapi (libusb)
    List,
    /// Identidade USB + mixers ALSA
    Info,
    /// Ping HID vendor 0x66 no dispositivo de áudio
    Ping,
    /// Iluminação do anel
    Led {
        #[command(subcommand)]
        cmd: LedCmd,
    },
    /// Mixers ALSA (mute, monitor, extension unit)
    Mixer {
        #[command(subcommand)]
        cmd: MixerCmd,
    },
    /// HID 0x66: polar, mute, high-pass, ganho, sidetone
    Hid {
        #[command(subcommand)]
        cmd: HidCmd,
    },
    /// AJAZZ AJ179 (HID 32 bytes, interface 2)
    #[command(name = "aj179")]
    Aj179 {
        #[command(subcommand)]
        cmd: Aj179Cmd,
    },
}

#[derive(Subcommand)]
enum LedCmd {
    Solid {
        /// Cor hex RRGGBB (ambos os anéis, se --top/--bottom omitidos)
        #[arg(default_value = "f20000")]
        color: String,
        #[arg(long)]
        top: Option<String>,
        #[arg(long)]
        bottom: Option<String>,
        #[arg(short, long, default_value_t = 100)]
        brightness: u8,
        /// Grava na memória do controlador
        #[arg(long)]
        save: bool,
        /// Reenvia o frame (~50 ms) até Ctrl+C
        #[arg(long)]
        hold: bool,
    },
    Off {
        #[arg(long)]
        save: bool,
    },
    Reset,
    /// Efeito gerado no host até Ctrl+C
    Effect {
        kind: EffectKind,
        #[arg(default_value = "f20000")]
        color: String,
        #[arg(short, long, default_value_t = 100)]
        brightness: u8,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum EffectKind {
    Blink,
    Cycle,
    Wave,
    Lightning,
    Pulse,
}

impl From<EffectKind> for Effect {
    fn from(k: EffectKind) -> Self {
        match k {
            EffectKind::Blink => Effect::Blink,
            EffectKind::Cycle => Effect::Cycle,
            EffectKind::Wave => Effect::Wave,
            EffectKind::Lightning => Effect::Lightning,
            EffectKind::Pulse => Effect::Pulse,
        }
    }
}

#[derive(Subcommand)]
enum MixerCmd {
    /// Dump `amixer scontents`
    Show,
    Mute {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// Volume de playback (monitor/sidetone no Linux)
    Monitor {
        percent: u8,
        #[arg(long)]
        mute: bool,
    },
    /// Extension Unit Realtek (bit USB; APO Windows não existe aqui)
    Extension {
        #[arg(value_enum)]
        state: OnOff,
    },
}

#[derive(Subcommand)]
enum HidCmd {
    Polar {
        #[arg(value_enum)]
        pattern: Option<PolarArg>,
    },
    Mute {
        #[arg(value_enum)]
        state: Option<OnOff>,
    },
    Highpass {
        #[arg(value_enum)]
        state: Option<OnOff>,
    },
    Gain {
        index: Option<u8>,
    },
    Sidetone {
        #[arg(value_enum)]
        mute: Option<OnOff>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum PolarArg {
    Cardioid,
    Omni,
}

impl From<PolarArg> for PolarPattern {
    fn from(p: PolarArg) -> Self {
        match p {
            PolarArg::Cardioid => PolarPattern::Cardioid,
            PolarArg::Omni => PolarPattern::Omnidirectional,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Subcommand)]
enum Aj179Cmd {
    /// Identidade, bateria e firmware do recetor
    Info,
    /// DPI, taxa, LED, sensor e energia (lidos do mouse)
    Show,
    Rate {
        hz: Option<u16>,
    },
    Lighting {
        #[arg(value_enum)]
        mode: Option<Aj179Light>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long, default_value_t = 4)]
        brightness: u8,
        #[arg(long, default_value_t = 2)]
        speed: u8,
    },
    /// Estágio 1–6 e valor (passos de 50)
    Dpi {
        stage: Option<u8>,
        value: Option<u16>,
    },
    /// Cor hex do estágio DPI
    DpiColour {
        stage: u8,
        color: String,
    },
    Button {
        index: Option<u8>,
        action: Option<String>,
    },
    ButtonsReset,
    /// Liga ou apaga o LED em todos os estágios DPI (as cores ficam gravadas à parte)
    Led {
        state: OnOff,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Aj179Light {
    Off,
    Breathing,
    Constant,
}

fn parse_rgb(s: &str) -> Result<Rgb> {
    Rgb::parse_hex(s).map_err(anyhow::Error::msg)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => {
            let api = HidApi::new()?;
            let found = enumerate(&api);
            let mice = aj179::enumerate(&api);
            if found.is_empty() && mice.is_empty() {
                println!("Nenhum periférico visível. Instale as regras udev e replugue o USB.");
            }
            for d in found.into_iter().chain(mice) {
                println!(
                    "{:04x}:{:04x} iface={} {}  {}",
                    d.vid, d.pid, d.interface, d.product, d.path
                );
            }
        }
        Cmd::Info => {
            let api = HidApi::new()?;
            println!("USB (hidapi/libusb):");
            for d in enumerate(&api) {
                println!(
                    "  {:04x}:{:04x} iface={} {} ({})",
                    d.vid, d.pid, d.interface, d.product, d.path
                );
            }
            println!("\nALSA:");
            match mixer::contents() {
                Ok(s) => print!("{s}"),
                Err(e) => println!("  ({e})"),
            }
            println!("\nHID 0x66 (não envie opcode 0x02 — reenumera o USB):");
            match AudioHid::open() {
                Ok(a) => {
                    match a.polar() {
                        Ok(p) => println!("  polar: {}", p.label()),
                        Err(e) => println!("  polar: ({e})"),
                    }
                    match a.muted() {
                        Ok(v) => println!("  mute: {v}"),
                        Err(e) => println!("  mute: ({e})"),
                    }
                    match a.highpass() {
                        Ok(v) => println!("  high-pass: {v}"),
                        Err(e) => println!("  high-pass: ({e})"),
                    }
                    match a.gain_index() {
                        Ok(v) => println!("  gain index: {v}"),
                        Err(e) => println!("  gain index: ({e})"),
                    }
                    match a.sidetone_muted() {
                        Ok(v) => println!("  sidetone mute: {v}"),
                        Err(e) => println!("  sidetone mute: ({e})"),
                    }
                }
                Err(e) => println!("  ({e})"),
            }
        }
        Cmd::Ping => {
            let audio = AudioHid::open()?;
            let r = audio.ping()?;
            println!(
                "ACK {}",
                r.iter()
                    .take(8)
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        Cmd::Led { cmd } => {
            let api = HidApi::new().context("hidapi")?;
            let led = Lighting::open(&api).context(
                "abra o controlador 03F0:098C iface 0 (libusb). Sem hidraw nesta interface.",
            )?;
            match cmd {
                LedCmd::Solid {
                    color,
                    top,
                    bottom,
                    brightness,
                    save,
                    hold,
                } => {
                    let base = parse_rgb(&color)?;
                    let top = top
                        .as_deref()
                        .map(parse_rgb)
                        .transpose()?
                        .unwrap_or(base)
                        .scaled(brightness);
                    let bottom = bottom
                        .as_deref()
                        .map(parse_rgb)
                        .transpose()?
                        .unwrap_or(base)
                        .scaled(brightness);
                    if save {
                        led.save_solid(top, bottom)?;
                    } else {
                        led.set_direct(top, bottom)?;
                    }
                    if hold {
                        hold_solid(&led, top, bottom)?;
                    }
                    println!("LED atualizado.");
                }
                LedCmd::Off { save } => {
                    led.off(save)?;
                    println!("LED apagado.");
                }
                LedCmd::Reset => {
                    led.firmware_reset()?;
                    println!("Reset de LED enviado (0xFD).");
                }
                LedCmd::Effect {
                    kind,
                    color,
                    brightness,
                } => {
                    let c = parse_rgb(&color)?;
                    run_effect(&led, kind.into(), c, brightness)?;
                }
            }
        }
        Cmd::Mixer { cmd } => match cmd {
            MixerCmd::Show => print!("{}", mixer::contents()?),
            MixerCmd::Mute { state } => {
                print!("{}", mixer::set_mute(matches!(state, OnOff::On))?);
            }
            MixerCmd::Monitor { percent, mute } => {
                print!("{}", mixer::set_monitor(percent, !mute)?);
            }
            MixerCmd::Extension { state } => {
                print!("{}", mixer::set_extension(matches!(state, OnOff::On))?);
            }
        },
        Cmd::Hid { cmd } => {
            let audio = AudioHid::open()?;
            match cmd {
                HidCmd::Polar { pattern } => match pattern {
                    Some(p) => {
                        audio.set_polar(p.into())?;
                        println!("{}", PolarPattern::from(p).label());
                    }
                    None => println!("{}", audio.polar()?.label()),
                },
                HidCmd::Mute { state } => match state {
                    Some(s) => {
                        let on = matches!(s, OnOff::On);
                        audio.set_muted(on)?;
                        println!("{}", if on { "muted" } else { "unmuted" });
                    }
                    None => println!("{}", if audio.muted()? { "muted" } else { "unmuted" }),
                },
                HidCmd::Highpass { state } => match state {
                    Some(s) => {
                        let amt = if matches!(s, OnOff::On) { 80 } else { 0 };
                        audio.set_highpass(amt)?;
                        println!("{amt}");
                    }
                    None => println!("{}", audio.highpass()?),
                },
                HidCmd::Gain { index } => match index {
                    Some(i) => {
                        audio.set_gain_index(i)?;
                        println!("{i}");
                    }
                    None => println!("{}", audio.gain_index()?),
                },
                HidCmd::Sidetone { mute } => match mute {
                    Some(s) => {
                        let on = matches!(s, OnOff::On);
                        audio.set_sidetone_muted(on)?;
                        println!("{}", if on { "muted" } else { "unmuted" });
                    }
                    None => {
                        println!(
                            "{}",
                            if audio.sidetone_muted()? {
                                "muted"
                            } else {
                                "unmuted"
                            }
                        )
                    }
                },
            }
        }
        Cmd::Aj179 { cmd } => {
            let mouse = Aj179::open().context("AJ179 interface 2 (hidraw)")?;
            match cmd {
                Aj179Cmd::Info | Aj179Cmd::Show => {
                    let s = mouse.snapshot();
                    if let Some(e) = &s.error {
                        println!("aviso: {e}");
                    }
                    println!("{:04x}:{:04x} {}  {}", s.vid, s.pid, s.link, s.path);
                    if let Some(i) = &s.info {
                        println!(
                            "id {}  sensor {:02x}  bateria {}%  {}  {}",
                            i.device_id,
                            i.sensor_id,
                            i.battery,
                            if i.charging { "a carregar" } else { "bateria" },
                            if i.connected { "acordado" } else { "a dormir" }
                        );
                    }
                    if matches!(cmd, Aj179Cmd::Show) {
                        if let Some(hz) = s.rate_hz {
                            println!("taxa {hz} Hz");
                        }
                        if !s.dpi.is_empty() {
                            let idx = s.dpi_index.unwrap_or(0);
                            let stages: Vec<String> = s
                                .dpi
                                .iter()
                                .enumerate()
                                .map(|(i, d)| {
                                    format!(
                                        "{}{} {}",
                                        d.value,
                                        if i as u8 == idx { "*" } else { "" },
                                        d.color.to_hex()
                                    )
                                })
                                .collect();
                            println!("dpi {}", stages.join(" | "));
                        } else {
                            let prefs = aj179_led::load();
                            if prefs.dpi_values.len() >= 2 {
                                println!(
                                    "dpi (guardado) {}",
                                    prefs
                                        .dpi_values
                                        .iter()
                                        .zip(
                                            prefs
                                                .dpi_colors
                                                .iter()
                                                .chain(std::iter::repeat(&"#000000".to_string()))
                                        )
                                        .map(|(v, c)| format!("{v} {c}"))
                                        .collect::<Vec<_>>()
                                        .join(" | ")
                                );
                            }
                        }
                        let prefs = aj179_led::load();
                        println!("led {}", if prefs.led_on { "on" } else { "off" });
                        if let Some(l) = &s.lighting {
                            println!(
                                "luz modo {} brilho {} velocidade {} {}",
                                l.mode,
                                l.brightness,
                                l.speed,
                                l.color.to_hex()
                            );
                        }
                        if let Some(p) = &s.sensor {
                            println!(
                                "lod {} mm  motion-sync {}  angle-snap {}",
                                p.lod, p.motion_sync, p.angle_snap
                            );
                        }
                        if let Some(p) = &s.power {
                            println!(
                                "sleep {}  wake-move {}  luz-ao-mover-off {}  debounce {}",
                                p.sleep, p.wake_on_move, p.lights_off_on_move, p.debounce
                            );
                        }
                        if let Some(fw) = &s.rx_fw {
                            println!("fw recetor {fw}");
                        }
                        for (i, code) in s.buttons.iter().enumerate() {
                            let name = aj179::BUTTON_NAMES.get(i).copied().unwrap_or("?");
                            println!("botão {} ({name}): {}", i + 1, aj179::button_action(*code));
                        }
                    }
                }
                Aj179Cmd::Rate { hz } => match hz {
                    Some(v) => {
                        mouse.set_rate(v)?;
                        println!("{v}");
                    }
                    None => {
                        let r = mouse.get(aj179::CMD_GET_RATE)?;
                        println!("{}", aj179::flag_to_rate(r[4]).unwrap_or(0));
                    }
                },
                Aj179Cmd::Lighting {
                    mode,
                    color,
                    brightness,
                    speed,
                } => match mode {
                    Some(m) => {
                        let rgb = color
                            .as_deref()
                            .map(parse_rgb)
                            .transpose()?
                            .unwrap_or(Rgb::new(0xff, 0, 0));
                        match m {
                            Aj179Light::Off => mouse.set_led_enabled(false)?,
                            Aj179Light::Breathing => {
                                mouse.set_led_enabled(true)?;
                                mouse.set_lighting(
                                    aj179::LIGHT_BREATHING,
                                    brightness,
                                    speed,
                                    rgb,
                                )?;
                            }
                            Aj179Light::Constant => {
                                mouse.set_led_enabled(true)?;
                                mouse.set_lighting(
                                    aj179::LIGHT_CONSTANT,
                                    brightness,
                                    speed,
                                    rgb,
                                )?;
                            }
                        }
                        println!("ok");
                    }
                    None => {
                        let r = mouse.get(aj179::CMD_GET_LIGHT)?;
                        if let Some(l) = aj179::parse_lighting(&r) {
                            println!(
                                "modo {} brilho {} velocidade {} {}",
                                l.mode,
                                l.brightness,
                                l.speed,
                                l.color.to_hex()
                            );
                        }
                    }
                },
                Aj179Cmd::Dpi { stage, value } => {
                    let snap = mouse.snapshot();
                    let sensor = snap
                        .info
                        .as_ref()
                        .map(|i| i.sensor_id)
                        .unwrap_or(aj179::SENSOR_PAW3395);
                    let mut stages: Vec<u16> = snap.dpi.iter().map(|d| d.value).collect();
                    if stages.is_empty() {
                        anyhow::bail!("DPI ainda não lido; mexa no mouse");
                    }
                    match (stage, value) {
                        (Some(st), Some(v)) => {
                            let i = stage_index(st, stages.len())?;
                            stages[i] = aj179::clamp_dpi(sensor, v);
                            mouse.set_dpi_stages(sensor, snap.dpi_index.unwrap_or(0), &stages)?;
                            println!("{}", stages[i]);
                        }
                        (Some(st), None) => {
                            let i = stage_index(st, stages.len())?;
                            println!("{}", stages[i]);
                        }
                        (None, None) => {
                            println!(
                                "{}",
                                stages
                                    .iter()
                                    .map(|v| v.to_string())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            );
                        }
                        (None, Some(_)) => anyhow::bail!("indique o estágio 1–6"),
                    }
                }
                Aj179Cmd::DpiColour { stage, color } => {
                    let snap = mouse.snapshot();
                    let mut colors: Vec<Rgb> = snap.dpi.iter().map(|d| d.color).collect();
                    if colors.is_empty() {
                        anyhow::bail!("cores DPI ainda não lidas; mexa no mouse");
                    }
                    let i = stage_index(stage, colors.len())?;
                    colors[i] = parse_rgb(&color)?;
                    mouse.set_dpi_colors(&colors)?;
                    println!("{}", colors[i].to_hex());
                }
                Aj179Cmd::Button { index, action } => {
                    let snap = mouse.snapshot();
                    let mut keys = snap.buttons;
                    if keys.len() < aj179::BUTTON_COUNT {
                        keys = aj179::FACTORY_BUTTONS.to_vec();
                    }
                    match (index, action) {
                        (Some(n), Some(act)) => {
                            let i = button_index(n)?;
                            if i == 0 {
                                anyhow::bail!("o botão esquerdo não é remapeado");
                            }
                            keys[i] = aj179::encode_button(&act)?;
                            mouse.set_buttons(&keys)?;
                            println!("{} → {act}", aj179::BUTTON_NAMES[i]);
                        }
                        (Some(n), None) => {
                            let i = button_index(n)?;
                            println!("{}", aj179::button_action(keys[i]));
                        }
                        (None, None) => {
                            for (i, code) in keys.iter().enumerate() {
                                println!(
                                    "{} {}: {}",
                                    i + 1,
                                    aj179::BUTTON_NAMES[i],
                                    aj179::button_action(*code)
                                );
                            }
                        }
                        (None, Some(_)) => anyhow::bail!("indique o botão 2–6"),
                    }
                }
                Aj179Cmd::ButtonsReset => {
                    mouse.set_buttons(&aj179::FACTORY_BUTTONS)?;
                    println!("fábrica");
                }
                Aj179Cmd::Led { state } => {
                    mouse.set_led_enabled(matches!(state, OnOff::On))?;
                    println!(
                        "{}",
                        if matches!(state, OnOff::On) {
                            "on"
                        } else {
                            "off"
                        }
                    );
                }
            }
        }
    }
    Ok(())
}

fn stage_index(stage: u8, len: usize) -> Result<usize> {
    if stage < 1 || stage as usize > len {
        anyhow::bail!("estágio DPI deve ser 1–{len}");
    }
    Ok((stage - 1) as usize)
}

fn button_index(n: u8) -> Result<usize> {
    if n < 1 || n as usize > aj179::BUTTON_COUNT {
        anyhow::bail!("botão deve ser 1–{}", aj179::BUTTON_COUNT);
    }
    Ok((n - 1) as usize)
}

fn hold_solid(led: &Lighting, top: Rgb, bottom: Rgb) -> Result<()> {
    println!("Hold ativo (Ctrl+C para sair).");
    loop {
        led.set_direct(top, bottom)?;
        thread::sleep(Duration::from_millis(40));
    }
}

fn run_effect(led: &Lighting, kind: Effect, color: Rgb, brightness: u8) -> Result<()> {
    println!("Efeito no host até Ctrl+C.");
    let start = Instant::now();
    loop {
        let t = start.elapsed().as_secs_f32();
        let (top, bottom) = frame(kind, t, color, brightness, 1.0);
        led.set_direct(top, bottom)?;
        thread::sleep(Duration::from_millis(40));
    }
}
