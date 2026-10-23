use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use duocast::device::{enumerate, AudioHid, Lighting};
use duocast::effects::{frame, Effect};
use duocast::mixer;
use duocast::protocol::{PolarPattern, Rgb};
use hidapi::HidApi;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    name = "duocast",
    about = "Controle Linux do HyperX DuoCast (LED HID + mixers ALSA)",
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

fn parse_rgb(s: &str) -> Result<Rgb> {
    Rgb::parse_hex(s).map_err(anyhow::Error::msg)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => {
            let api = HidApi::new()?;
            let found = enumerate(&api);
            if found.is_empty() {
                println!(
                    "Nenhum DuoCast visível. Instale udev/99-hyperx-duocast.rules e replugue."
                );
            }
            for d in found {
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
    }
    Ok(())
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
