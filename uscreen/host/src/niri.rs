//! niri: switching the virtual output on and off and saying where it sits.
//!
//! The job is smaller than on Hyprland: niri has no per-device touch/pen
//! mapping at all (an absolute device always addresses the whole desktop), so
//! everything here is about the output — enable it, place it, report its
//! rectangle on the desktop so the injector can scale tablet coordinates into
//! it. That last part replaces what `hyprctl keyword device[...]:output` does
//! on Hyprland.
//!
//! All of it goes through `niri msg`: the compositor's own IPC, no config
//! edits, and a user rule for the output (in config.kdl) always wins because
//! we only touch an output the compositor left switched off.
//!
//! Output creation belongs to the virtual-output patch (upstream PR #3800):
//! `niri msg create-virtual-output`. When that is absent (stock 26.04) the
//! output has to come from somewhere else — a forced DRM connector — and this
//! module only switches it on and off.

use tracing::{info, warn};

/// Whether this is a niri session we can talk to.
pub fn active() -> bool {
    let Some(sock) = std::env::var_os("NIRI_SOCKET") else {
        return false;
    };
    std::os::unix::net::UnixStream::connect(sock).is_ok()
}

async fn msg(args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new("niri")
        .arg("msg")
        .args(args)
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if !err.trim().is_empty() {
            warn!("niri msg {}: {}", args.join(" "), err.trim());
        }
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run a `niri msg` command for its side effect, logging whether it took.
async fn run(args: &[&str]) -> bool {
    match msg(args).await {
        Some(_) => {
            info!("niri msg {}: ok", args.join(" "));
            true
        }
        None => false,
    }
}

#[derive(Debug, Clone)]
pub struct Monitor {
    pub name: String,
    /// Where the output's logical rectangle starts, in layout coordinates.
    pub x: f64,
    pub y: f64,
    /// Logical size (mode size divided by scale); 0x0 while switched off.
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    /// Largest advertised mode in pixels, e.g. 3000x1872.
    pub best_mode: Option<(u32, u32)>,
}

impl Monitor {
    /// A niri output that is switched off reports no logical rectangle at all.
    pub fn is_on(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    /// The box this monitor covers on the desktop, in layout coordinates.
    pub fn logical(&self) -> (f64, f64, f64, f64) {
        (self.x, self.y, self.width, self.height)
    }
}

/// Every output niri knows, switched on or not.
pub async fn monitors() -> Option<Vec<Monitor>> {
    parse_outputs(&msg(&["--json", "outputs"]).await?)
}

fn parse_outputs(json: &str) -> Option<Vec<Monitor>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut out = Vec::new();
    for (name, o) in v.as_object()? {
        let logical = o.get("logical").filter(|l| !l.is_null());
        let num = |src: &serde_json::Value, k: &str| {
            src.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0)
        };
        let (x, y, width, height, scale) = match logical {
            Some(l) => (
                num(l, "x"),
                num(l, "y"),
                num(l, "width"),
                num(l, "height"),
                num(l, "scale").max(0.01),
            ),
            None => (0.0, 0.0, 0.0, 0.0, 1.0),
        };
        let best_mode = o
            .get("modes")
            .and_then(|m| m.as_array())
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some((
                    m.get("width")?.as_u64()? as u32,
                    m.get("height")?.as_u64()? as u32,
                ))
            })
            .max_by_key(|(w, h)| w * h);
        out.push(Monitor {
            name: o
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or(name)
                .to_string(),
            x,
            y,
            width,
            height,
            scale,
            best_mode,
        });
    }
    Some(out)
}

/// A scale from the config file, when the user pinned one (`scale = 1.5`).
/// Read here rather than plumbed through: it changes the output's logical
/// size, nothing about the capture pipeline.
fn configured_scale() -> Option<f64> {
    let raw = crate::config::FileConfig::load().scale?;
    if raw.eq_ignore_ascii_case("auto") {
        return None;
    }
    raw.trim().parse::<f64>().ok().filter(|s| (0.5..=4.0).contains(s))
}

/// An integer scale, so the logical size is whole: 2 for a tablet-class panel
/// (its desktop is still at least 1280 wide), 1 otherwise. A user rule in
/// config.kdl for the output overrides whatever we pick here.
fn scale_for(w: u32, h: u32) -> f64 {
    if w.is_multiple_of(2) && h.is_multiple_of(2) && w / 2 >= 1280 {
        2.0
    } else {
        1.0
    }
}

/// Where an output should start so that it sits on the requested side of the
/// other screens. The compositor only takes absolute coordinates.
fn place_on(all: &[Monitor], mine: &str, mode: (u32, u32), scale: f64, position: crate::config::Position) -> (f64, f64) {
    let others: Vec<(f64, f64, f64, f64)> = all
        .iter()
        .filter(|m| m.name != mine && m.is_on())
        .map(Monitor::logical)
        .collect();
    let (w, h) = (mode.0 as f64 / scale, mode.1 as f64 / scale);
    let min_x = others.iter().map(|b| b.0).fold(0.0, f64::min);
    let min_y = others.iter().map(|b| b.1).fold(0.0, f64::min);
    let max_x = others.iter().map(|b| b.0 + b.2).fold(0.0, f64::max);
    let max_y = others.iter().map(|b| b.1 + b.3).fold(0.0, f64::max);
    match position {
        crate::config::Position::Right => (max_x, min_y),
        crate::config::Position::Left => (min_x - w, min_y),
        crate::config::Position::Above => (min_x, min_y - h),
        crate::config::Position::Below => (min_x, max_y),
    }
}

/// Whether niri lists this output at all, switched on or not.
pub async fn output_exists(name: &str) -> bool {
    monitors()
        .await
        .is_some_and(|all| all.iter().any(|m| m.name == name))
}

/// Switch one of `names` on if niri left it off, placed on the requested side
/// of the other screens. Called on every pass of the capture loop, so it does
/// nothing at all while the output is already on.
pub async fn enable_output(names: &[String], position: crate::config::Position) {
    // The output appears a moment after whatever created it, so give it a
    // second before deciding it was left off.
    let mut seen_off = 0;
    for attempt in 0..15 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let Some(all) = monitors().await else { continue };
        let Some(mon) = all.iter().find(|m| names.contains(&m.name)) else {
            continue;
        };
        if mon.is_on() {
            // Already up: leave it alone, except for a scale the user pinned
            // in the config file, which the compositor cannot know about, and
            // the side it should sit on. Position used to be applied only on
            // the switch-on path below, so changing it while the output was
            // on did nothing until it was turned off and on again.
            let want_scale = configured_scale().unwrap_or(mon.scale);
            if (mon.scale - want_scale).abs() > 0.01 {
                info!("setting {} scale to {}", mon.name, want_scale);
                run(&["output", &mon.name, "scale", &format!("{want_scale}")]).await;
            }
            let mode = mon
                .best_mode
                .unwrap_or((mon.width as u32, mon.height as u32));
            let (x, y) = place_on(&all, &mon.name, mode, want_scale, position);
            if (mon.x - x).abs() > 0.5 || (mon.y - y).abs() > 0.5 {
                info!("placing {} at {},{} ({:?})", mon.name, x, y, position);
                run(&[
                    "output",
                    &mon.name,
                    "position",
                    "set",
                    // `--` so clap does not take a negative coordinate for a
                    // flag: left/above place the output at -1600 or -1000.
                    "--",
                    &format!("{x}"),
                    &format!("{y}"),
                ])
                .await;
            }
            return;
        }
        seen_off += 1;
        if seen_off < 5 {
            continue;
        }

        let mode = mon.best_mode.unwrap_or((1920, 1200));
        let scale = configured_scale().unwrap_or_else(|| scale_for(mode.0, mode.1));
        let name = mon.name.clone();
        info!("niri left {} off — switching it on", name);
        run(&["output", &name, "on"]).await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        run(&["output", &name, "scale", &format!("{scale}")]).await;
        // Placing needs the other screens, which we just read; the output is
        // on by now, so the position is applied against the current layout.
        let (x, y) = place_on(&all, &name, mode, scale, position);
        run(&[
            "output",
            &name,
            "position",
            "set",
            // See above: negative coordinates need the end-of-options marker.
            "--",
            &format!("{x}"),
            &format!("{y}"),
        ])
        .await;

        for _ in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let on = monitors()
                .await
                .and_then(|all| all.into_iter().find(|m| m.name == name))
                .is_some_and(|m| m.is_on());
            if on {
                return;
            }
        }
        warn!(
            "niri did not switch {} on. By hand: niri msg output {} on, then \
             niri msg output {} mode {}x{}",
            name, name, name, mode.0, mode.1
        );
        return;
    }
    warn!("the virtual output did not appear in `niri msg outputs` within 3s");
}

/// Switch the output off so niri moves its workspaces back to the real
/// screens. The next `enable_output` switches it on again.
pub async fn disable_output(names: &[String], virtual_output: bool) {
    for name in names {
        // A virtual output can be removed outright: then it costs nothing at
        // all, which is the point of having it on demand. A physical (or
        // forced) connector can only be switched off.
        if virtual_output {
            // Asking to remove an output that was never created is a warning
            // on every start (no tablet attached yet), not a fault.
            if output_exists(name).await && run(&["remove-virtual-output", name]).await {
                continue;
            } else if !output_exists(name).await {
                continue;
            }
        }
        info!("disabling niri output {}", name);
        run(&["output", name, "off"]).await;
    }
}

/// Create a virtual output (upstream PR #3800) and return its name. `None`
/// when this niri predates the feature.
///
/// Used by the DMA-BUF capture backend: there the output is the tablet's whole
/// existence, so it is created when the stream starts and removed when it
/// stops, instead of sitting in the layout permanently.
#[allow(dead_code)]
pub async fn create_virtual(name: &str, width: u32, height: u32, refresh: u32) -> Option<String> {
    let (w, h, r) = (width.to_string(), height.to_string(), refresh.to_string());
    let args = [
        "create-virtual-output",
        "--name",
        name,
        "--width",
        w.as_str(),
        "--height",
        h.as_str(),
        "--refresh-rate",
        r.as_str(),
    ];
    let out = msg(&args).await?;
    info!("niri created virtual output {}: {}", name, out);
    Some(name.to_string())
}

/// Give an existing output a new mode (virtual outputs accept custom modes,
/// so the tablet's resolution can change without recreating the output).
pub async fn set_custom_mode(name: &str, width: u32, height: u32, refresh: u32) -> bool {
    run(&[
        "output",
        name,
        "custom-mode",
        &format!("{width}x{height}@{refresh}"),
    ])
    .await
}

/// The first of `names` that is switched on, waiting up to `timeout`.
pub async fn wait_on(names: &[String], timeout: std::time::Duration) -> Option<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(m) = monitors()
            .await
            .and_then(|all| all.into_iter().find(|m| names.contains(&m.name) && m.is_on()))
        {
            return Some(m.name);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// The screen the user is looking at, for graphics-tablet mode: the focused
/// output unless it is one of ours, else the first real one that is on.
pub async fn primary_output(ours: &[String]) -> Option<String> {
    let focused = msg(&["--json", "focused-output"])
        .await
        .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
        .and_then(|v| v.get("name")?.as_str().map(str::to_string));
    let all = monitors().await?;
    let real: Vec<&Monitor> = all
        .iter()
        .filter(|m| m.is_on() && !ours.contains(&m.name))
        .collect();
    real.iter()
        .find(|m| Some(&m.name) == focused.as_ref())
        .or_else(|| real.first())
        .map(|m| m.name.clone())
}

/// Where `output` sits on the desktop, as fractions of the whole layout's
/// bounding box: (x, y, width, height). Same contract as the KDE/Hyprland
/// versions, except that the injector uses it for touch and pen too.
pub async fn output_area(output: &str) -> Option<(f64, f64, f64, f64)> {
    let all = monitors().await?;
    let on: Vec<(f64, f64, f64, f64)> = all
        .iter()
        .filter(|m| m.is_on())
        .map(Monitor::logical)
        .collect();
    let (x, y, w, h) = all.iter().find(|m| m.name == output && m.is_on())?.logical();
    let min_x = on.iter().map(|b| b.0).fold(f64::INFINITY, f64::min);
    let min_y = on.iter().map(|b| b.1).fold(f64::INFINITY, f64::min);
    let max_x = on.iter().map(|b| b.0 + b.2).fold(f64::NEG_INFINITY, f64::max);
    let max_y = on.iter().map(|b| b.1 + b.3).fold(f64::NEG_INFINITY, f64::max);
    let (tw, th) = (max_x - min_x, max_y - min_y);
    (tw > 0.0 && th > 0.0).then(|| ((x - min_x) / tw, (y - min_y) / th, w / tw, h / th))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped like `niri msg --json outputs`: a laptop panel on at 0,0 and a
    /// tablet-sized output that was created but never switched on.
    const JSON: &str = r#"{
      "eDP-1": {
        "name": "eDP-1",
        "modes": [{"width":1920,"height":1080,"refresh_rate":60008,"is_preferred":true}],
        "current_mode": 0,
        "logical": {"x":0,"y":0,"width":1920,"height":1080,"scale":1.0,"transform":"Normal"}
      },
      "tablet": {
        "name": "tablet",
        "modes": [{"width":3000,"height":1872,"refresh_rate":59972,"is_preferred":true}],
        "current_mode": 0,
        "logical": null
      }
    }"#;

    #[test]
    fn reads_an_output_niri_left_off() {
        let m = parse_outputs(JSON).unwrap();
        let tablet = m.iter().find(|m| m.name == "tablet").unwrap();
        assert!(!tablet.is_on());
        assert_eq!(tablet.best_mode, Some((3000, 1872)));
        let edp = m.iter().find(|m| m.name == "eDP-1").unwrap();
        assert!(edp.is_on());
    }

    #[test]
    fn places_the_output_beside_the_others() {
        let m = parse_outputs(JSON).unwrap();
        // 3000x1872 at scale 2 is 1500x936 logical, to the right of 1920x1080.
        assert_eq!(
            place_on(&m, "tablet", (3000, 1872), 2.0, crate::config::Position::Right),
            (1920.0, 0.0)
        );
        assert_eq!(
            place_on(&m, "tablet", (3000, 1872), 2.0, crate::config::Position::Below),
            (0.0, 1080.0)
        );
        assert_eq!(
            place_on(&m, "tablet", (3000, 1872), 1.0, crate::config::Position::Left),
            (-3000.0, 0.0)
        );
    }

    #[test]
    fn scale_is_whole_and_tablet_sized() {
        assert_eq!(scale_for(3000, 1872), 2.0);
        assert_eq!(scale_for(2400, 1500), 1.0);
        assert_eq!(scale_for(1920, 1200), 1.0);
        assert_eq!(scale_for(2561, 1600), 1.0);
    }
}
