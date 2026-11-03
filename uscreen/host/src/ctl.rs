//! The daemon's control socket: the screen switch and the settings.
//!
//! There is no window and no tray icon any more — the DMS widget is the whole
//! interface — so everything the user can change has to be reachable from
//! outside the process. That is one Unix socket in the runtime directory:
//!
//! * `on`, `off`, `toggle`, `status` — the second screen itself. Off means the
//!   virtual output is removed, so nothing is rendered, captured or encoded.
//! * `config` — every setting in `config.toml`, read or written, applied to
//!   the running pipeline. Writing through the socket means no `systemctl
//!   restart` and no black frame.
//! * `mode WxH@FPS` — the one compound change: the virtual output's size and
//!   refresh, at the top level and in the tablets' profiles, applied live.
//!
//! A change that only the pipeline understands (encoder, fps, bitrate, size,
//! quality, scale) goes through the settings watch channel the capture manager
//! already follows; a structural one (capture backend, output name, cursor,
//! side) also re-sends the settings, and the DMA-BUF backend rebuilds its
//! pipeline and re-reads those keys from the file when it starts.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::capture::EncoderSettings;
use crate::config::FileConfig;

static DISPLAY: std::sync::OnceLock<watch::Sender<bool>> = std::sync::OnceLock::new();
static SETTINGS: std::sync::OnceLock<watch::Sender<EncoderSettings>> = std::sync::OnceLock::new();

fn sender() -> watch::Sender<bool> {
    DISPLAY.get_or_init(|| watch::channel(true).0).clone()
}

/// A subscription to the on/off switch, for whoever gates a pipeline on it.
/// The switch starts ON: the daemon launching is not the user turning the
/// screen off, and the tablet-attached gate still decides whether anything
/// runs at all.
pub fn display() -> watch::Receiver<bool> {
    DISPLAY.get_or_init(|| watch::channel(true).0).subscribe()
}

/// Start listening, once per process.
pub fn spawn_server(settings: watch::Sender<EncoderSettings>) {
    SETTINGS.get_or_init(|| settings);
    let tx = sender();
    tokio::spawn(async move {
        if let Err(e) = serve(tx).await {
            warn!("control socket unavailable: {e:#}");
        }
    });
}

fn socket_path() -> PathBuf {
    crate::runtime::runtime_dir().join("control.sock")
}

/// Daemon side: listen and answer.
pub async fn serve(display_tx: watch::Sender<bool>) -> Result<()> {
    let path = socket_path();
    // A stale socket from a killed daemon would make bind() fail with
    // "address in use" for a socket nobody is listening on.
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("binding the control socket {}", path.display()))?;
    info!("control socket at {}", path.display());

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                warn!("control socket accept failed: {e}");
                continue;
            }
        };
        let tx = display_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, tx).await {
                warn!("control connection: {e:#}");
            }
        });
    }
}

async fn handle(stream: UnixStream, tx: watch::Sender<bool>) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let reply: String = match answer(&line, &tx).await {
            Ok(reply) => reply,
            Err(e) => format!("error: {e:#}"),
        };
        write.write_all(reply.as_bytes()).await?;
        write.write_all(b"\n\n").await?; // blank line = end of reply
        write.flush().await?;
    }
    Ok(())
}

async fn answer(line: &str, tx: &watch::Sender<bool>) -> Result<String> {
    let line = line.trim();
    let (command, rest) = match line.split_once(' ') {
        Some((c, r)) => (c, r.trim()),
        None => (line, ""),
    };

    match command {
        "" => Ok(String::new()),
        "on" => {
            tx.send(true).ok();
            Ok("on".into())
        }
        "off" => {
            tx.send(false).ok();
            Ok("off".into())
        }
        "toggle" => {
            let next = !*tx.borrow();
            tx.send(next).ok();
            Ok(if next { "on" } else { "off" }.into())
        }
        "status" => Ok(if *tx.borrow() { "on" } else { "off" }.into()),
        "config" if rest.starts_with("mode ") => set_mode(rest["mode ".len()..].trim()).map(|_| describe()),
        "config" => match rest.split_once(' ') {
            None if rest.is_empty() || rest == "get" => Ok(describe()),
            Some(("set", kv)) => {
                let (key, value) = kv
                    .split_once(' ')
                    .with_context(|| format!("`config set` needs a key and a value, got {kv:?}"))?;
                set_key(key.trim(), value.trim()).map(|_| describe())
            }
            Some(("get", _)) | None => Ok(describe()),
            Some((other, _)) => bail!("unknown config command {other:?}: use get or set"),
        },
        "mode" => set_mode(rest).map(|_| describe()),
        other => bail!("unknown command {other:?}: on, off, toggle, status, config, mode"),
    }
}

/// Every setting the interface can change, as `key = value` lines.
fn describe() -> String {
    let c = FileConfig::load();
    let line = |k: &str, v: String| format!("{k} = {v}");
    [
        line("capture", c.capture.clone().unwrap_or_else(|| "evdi".into())),
        line(
            "output_name",
            c.output_name.clone().unwrap_or_else(|| "tablet".into()),
        ),
        line("width", c.width.to_string()),
        line("height", c.height.to_string()),
        line("fps", c.fps.to_string()),
        line("bitrate", c.bitrate.to_string()),
        line("quality", c.quality.to_string()),
        line("encoder", c.encoder.clone()),
        line("stream_scale", c.stream_scale.to_string()),
        line("scale", c.scale.clone().unwrap_or_else(|| "auto".into())),
        line("position", c.position.clone()),
        line(
            "cursor",
            c.cursor.unwrap_or(true).to_string(),
        ),
        line("auto_resolution", c.auto_resolution.to_string()),
        line("ten_bit", c.ten_bit.to_string()),
        line("auto_launch_app", c.auto_launch_app.to_string()),
        line("pen_only", c.pen_only.to_string()),
        line("tray", c.tray.unwrap_or(false).to_string()),
    ]
    .join("\n")
}

fn parse_bool(v: &str) -> Result<bool> {
    match v {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        other => bail!("expected true or false, got {other:?}"),
    }
}

fn set_key(key: &str, value: &str) -> Result<()> {
    let mut cfg = FileConfig::load();
    match key {
        "capture" => {
            if !matches!(value, "wayland" | "evdi" | "dmabuf" | "dmabuf-wayland") {
                bail!("capture must be wayland or evdi");
            }
            cfg.capture = Some(value.to_string());
        }
        "output_name" => cfg.output_name = Some(value.to_string()),
        "width" => cfg.width = value.parse().context("width")?,
        "height" => cfg.height = value.parse().context("height")?,
        "fps" => cfg.fps = value.parse().context("fps")?,
        "bitrate" => cfg.bitrate = value.parse().context("bitrate")?,
        "quality" => cfg.quality = value.parse().context("quality")?,
        "encoder" => cfg.encoder = value.to_string(),
        "stream_scale" => cfg.stream_scale = value.parse().context("stream_scale")?,
        "scale" => cfg.scale = Some(value.to_string()),
        "position" => {
            if crate::config::Position::parse(value).is_none() {
                bail!("position must be right, left, above or below");
            }
            cfg.position = value.to_string();
        }
        "cursor" => cfg.cursor = Some(parse_bool(value)?),
        "auto_resolution" => cfg.auto_resolution = parse_bool(value)?,
        "ten_bit" => cfg.ten_bit = parse_bool(value)?,
        "auto_launch_app" => cfg.auto_launch_app = parse_bool(value)?,
        "pen_only" => cfg.pen_only = parse_bool(value)?,
        "tray" => cfg.tray = Some(parse_bool(value)?),
        other => bail!("unknown key {other:?}"),
    }
    cfg.save().context("writing config.toml")?;
    push_settings(&cfg);
    Ok(())
}

/// `mode 2400x1500@60` — the composite change a resolution preset makes: the
/// top-level keys *and* every remembered tablet profile, because a profile
/// wins over the top level when that tablet connects.
fn set_mode(spec: &str) -> Result<()> {
    let (size, refresh) = spec
        .split_once('@')
        .with_context(|| format!("expected WxH@FPS, got {spec:?}"))?;
    let (w, h) = size
        .split_once(['x', 'X'])
        .with_context(|| format!("expected WxH@FPS, got {spec:?}"))?;
    let (width, height, fps): (u32, u32, u32) = (
        w.trim().parse().context("width")?,
        h.trim().parse().context("height")?,
        refresh.trim().parse().context("fps")?,
    );
    let mut cfg = FileConfig::load();
    cfg.width = width;
    cfg.height = height;
    cfg.fps = fps;
    for profile in cfg.tablets.values_mut() {
        profile.width = width;
        profile.height = height;
    }
    cfg.save().context("writing config.toml")?;
    push_settings(&cfg);
    Ok(())
}

/// Hand the new settings to the running pipeline. The capture manager already
/// watches this channel and restarts only what the change needs; a structural
/// change (cursor, backend, side, output name) makes the DMA-BUF backend
/// rebuild its pipeline, which is where it re-reads those keys from the file.
fn push_settings(cfg: &FileConfig) {
    let Some(tx) = SETTINGS.get() else { return };
    let mut s = tx.borrow().clone();
    s.encoder = cfg.encoder.clone();
    s.fps = cfg.fps;
    s.bitrate = cfg.bitrate;
    s.width = cfg.width;
    s.height = cfg.height;
    s.quality = cfg.quality;
    s.stream_scale = cfg.stream_scale;
    let _ = tx.send(s);
}

/// Client side: one command, one answer.
pub async fn request(command: &str) -> Result<String> {
    let path = socket_path();
    let stream = UnixStream::connect(&path).await.with_context(|| {
        format!(
            "the uscreen daemon is not running (no control socket at {})",
            path.display()
        )
    })?;
    let (read, mut write) = stream.into_split();
    write.write_all(command.as_bytes()).await?;
    write.write_all(b"\n").await?;
    write.flush().await?;

    let mut answer = String::new();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        if line.is_empty() {
            break;
        }
        answer.push_str(&line);
        answer.push('\n');
    }
    let answer = answer.trim_end().to_string();
    if answer.starts_with("error:") {
        bail!("{}", answer["error:".len()..].trim());
    }
    Ok(answer)
}
