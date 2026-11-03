//! Turning the tablet screen on and off from outside the daemon.
//!
//! The daemon already knows how to raise and tear down the virtual output — it
//! does it whenever a tablet is plugged in or switched to pen-only mode. What
//! was missing is a way for the *user* to say "not now" without unplugging the
//! cable or stopping the service: a bar widget, a keybinding, a script.
//!
//! That is one boolean over a Unix socket in the runtime directory. It is
//! additive to the two conditions that already gate the display, so flipping it
//! off removes the output (nothing is rendered, captured or encoded — the point
//! of the whole exercise) and flipping it on brings it back with the same
//! settings, without restarting anything.

use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tracing::{info, warn};

static DISPLAY: std::sync::OnceLock<watch::Sender<bool>> = std::sync::OnceLock::new();

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
pub fn spawn_server() {
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

/// Daemon side: listen for `on`, `off`, `toggle`, `status` and answer with the
/// resulting state.
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
        let reply: String = match line.trim() {
            "on" => {
                tx.send(true).ok();
                "on".into()
            }
            "off" => {
                tx.send(false).ok();
                "off".into()
            }
            "toggle" => {
                let next = !*tx.borrow();
                tx.send(next).ok();
                if next { "on".into() } else { "off".into() }
            }
            "status" => {
                if *tx.borrow() { "on".into() } else { "off".into() }
            }
            "" => continue,
            other => format!("error: expected on, off, toggle or status, got {other:?}"),
        };
        write.write_all(reply.as_bytes()).await?;
        write.write_all(b"\n").await?;
        write.flush().await?;
    }
    Ok(())
}

/// Client side: one command, one answer. `status` is what a widget polls when
/// it has nothing better to do; it never changes the state.
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

    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let answer = line.trim().to_string();
    if let Some(rest) = answer.strip_prefix("error:") {
        anyhow::bail!("{}", rest.trim());
    }
    Ok(answer)
}
