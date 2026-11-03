//! DMA-BUF capture backend: the compositor's own frames, encoded on the GPU.
//!
//! The evdi path exists because evdi can only hand out system memory: the
//! driver copies the frame, we convert it on the CPU, and at 3000x1872 that
//! cost 154 % of a core and capped the stream at ~32 fps (see the note in
//! `capture.rs`). A compositor that implements `ext-image-copy-capture` hands
//! out the frame as a DMA-BUF instead — the same buffer it rendered into — so
//! the encoder reads it where it lies. Measured on this laptop (Iris Xe,
//! 2400x1500@60): capture 0.5 % CPU, encode ~8 %, end-to-end pacing 16.7 ms.
//!
//! Requires niri with the virtual-output patch (or any output, e.g. a forced
//! DRM connector): `niri::create_virtual` / `niri::enable_output` own the
//! output, this module owns the pixels.
//!
//! Frames arrive already in AR24, not XR24: Intel's `vapostproc` lists AR24 in
//! its DMA-BUF caps and XR24 not at all, so asking for XR24 produces
//! `not-negotiated` and, worse, a LINEAR fallback the encoder also refuses.

use anyhow::{Context, Result};
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use tracing::{info, warn};

use crate::capture::{Codec, EncoderSettings, H264AnnexBPacketizer, VideoPacket};

/// Everything that decides the shape of one pipeline run.
#[derive(Clone, Debug)]
pub struct BackendConfig {
    pub output: String,
    /// Create the output when it does not exist and remove it again when the
    /// stream stops (virtual output), or merely switch an existing one on and
    /// off (a forced DRM connector).
    pub virtual_output: bool,
    pub position: crate::config::Position,
    /// Composite the cursor into captured frames. Without it the pointer is
    /// invisible on the tablet while still clicking — the compositor hands the
    /// cursor out separately in that case.
    pub cursor: bool,
}

pub async fn stream_frames(
    cfg: BackendConfig,
    tx: broadcast::Sender<VideoPacket>,
    mut settings_rx: watch::Receiver<EncoderSettings>,
    mut display_rx: watch::Receiver<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
    codec_config: Arc<Mutex<Option<bytes::Bytes>>>,
    latency: crate::latency::LatencyTracker,
) -> Result<()> {
    let mut error_backoff = Duration::from_millis(500);

    loop {
        if *shutdown_rx.borrow() {
            remove_output(&cfg).await;
            return Ok(());
        }

        let settings = settings_rx.borrow_and_update().clone();

        // No tablet is being used as a screen: the output must not exist, so
        // that nothing is rendered or encoded while the user is not looking.
        if !*display_rx.borrow() {
            info!("no tablet attached — removing the virtual output");
            remove_output(&cfg).await;
            tokio::select! {
                _ = display_rx.changed() => continue,
                _ = settings_rx.changed() => continue,
                _ = shutdown_rx.changed() => continue,
            }
        }

        // Create the output if needed, then switch it on and place it.
        if cfg.virtual_output && crate::niri::output_exists(&cfg.output).await {
            // The output outlived a settings change: give it the new mode
            // rather than tearing it down and building it again.
            crate::niri::set_custom_mode(
                &cfg.output,
                settings.width,
                settings.height,
                settings.fps,
            )
            .await;
        }
        if cfg.virtual_output && !crate::niri::output_exists(&cfg.output).await {
            if crate::niri::create_virtual(
                &cfg.output,
                settings.width,
                settings.height,
                settings.fps,
            )
            .await
            .is_none()
            {
                anyhow::bail!(
                    "niri did not create the virtual output — this niri predates \
                     `create-virtual-output` (upstream PR #3800)"
                );
            }
        }
        let names = [cfg.output.clone()];
        crate::niri::enable_output(&names, cfg.position).await;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let cfg_thread = cfg.clone();
        let settings_thread = settings.clone();
        let tx_thread = tx.clone();
        let codec_thread = codec_config.clone();
        let latency_thread = latency.clone();

        let mut handle = tokio::task::spawn_blocking(move || {
            run_pipeline(
                &cfg_thread,
                &settings_thread,
                tx_thread,
                codec_thread,
                latency_thread,
                stop_thread,
            )
        });

        let result = tokio::select! {
            r = &mut handle => Some(r),
            _ = settings_rx.changed() => None,
            _ = display_rx.changed() => None,
            _ = shutdown_rx.changed() => None,
        };

        match result {
            Some(joined) => {
                match joined {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        // A missing protocol or a rejected modifier is not
                        // going to fix itself; say what it was and stop rather
                        // than spinning on it.
                        error_backoff = error_backoff.min(Duration::from_secs(5));
                        warn!("DMA-BUF capture stopped: {e:#}");
                        remove_output(&cfg).await;
                        tokio::select! {
                            _ = tokio::time::sleep(error_backoff) => {}
                            _ = shutdown_rx.changed() => { return Ok(()); }
                            _ = display_rx.changed() => {}
                        }
                        error_backoff = (error_backoff * 2).min(Duration::from_secs(5));
                        continue;
                    }
                    Err(e) => warn!("capture thread panicked: {e}"),
                }
                error_backoff = Duration::from_millis(500);
            }
            None => {
                // Settings, the tablet or the daemon asked us to stop: let the
                // blocking loop finish the frame it is on (one frame period at
                // most) and then rebuild with the new settings.
                stop.store(true, Ordering::Relaxed);
                let _ = handle.await;
                if *shutdown_rx.borrow() {
                    remove_output(&cfg).await;
                    return Ok(());
                }
            }
        }
    }
}

async fn remove_output(cfg: &BackendConfig) {
    if cfg.virtual_output {
        crate::niri::disable_output(&[cfg.output.clone()], true).await;
    } else {
        crate::niri::disable_output(&[cfg.output.clone()], false).await;
    }
}

/// The blocking half: capture a frame, hand it to the GPU encoder, ship the
/// packets. One frame at a time, which is what keeps the latency at one frame
/// period instead of letting a queue build up.
fn run_pipeline(
    cfg: &BackendConfig,
    settings: &EncoderSettings,
    tx: broadcast::Sender<VideoPacket>,
    codec_config: Arc<Mutex<Option<bytes::Bytes>>>,
    latency: crate::latency::LatencyTracker,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let modifiers = mbuf_encoder::supported_modifiers(mbuf_capture::AR24);
    info!(
        "DMA-BUF capture of {} with modifiers {:x?} ({:?})",
        cfg.output, modifiers, settings.encoder
    );

    let cap = mbuf_capture::session::Capture::new(
        &cfg.output,
        &mbuf_capture::session::CaptureConfig {
            mode: mbuf_capture::session::BufferMode::Dmabuf,
            // Three buffers: one in the encoder, one in the compositor, one
            // being filled — the minimum that never stalls a capture request.
            pool_size: 3,
            allowed_modifiers: modifiers,
            paint_cursor: cfg.cursor,
        },
    )
    .context("opening a DMA-BUF capture session")?;

    let fourcc = mbuf_capture::AR24;
    let modifier = cap.modifier.unwrap_or(0);
    info!(
        "capture session: {}x{} fourcc AR24 modifier 0x{modifier:016x}",
        cap.width, cap.height
    );

    if settings.encoder.to_ascii_lowercase().contains("hevc")
        || settings.encoder.contains("265")
    {
        warn!(
            "the DMA-BUF backend encodes H.264 only for now — ignoring the \
             configured encoder {}",
            settings.encoder
        );
    }

    let encoder = mbuf_encoder::Encoder::new(&mbuf_encoder::EncoderConfig {
        width: cap.width,
        height: cap.height,
        framerate: settings.fps.max(1),
        bitrate_kbps: settings.bitrate,
        fourcc,
        modifier,
        // One keyframe a second. USB is lossless so they buy little, but the
        // tablet has no way to ask for one: this is what bounds how long it
        // takes a (re)connecting client to get a picture.
        keyframe_interval: settings.fps.max(1),
        target_usage: 3,
        rate_control: "vbr".to_string(),
        cabac: true,
        num_slices: 1,
    })
    .context("starting the VA-API encoder")?;

    // Two threads, a bounded queue: capture blocks on the compositor's next
    // frame, the encoder takes ~10 ms. Serialising them costs one frame of the
    // 16.7 ms budget per frame, which is exactly the 60 -> 45 fps measured
    // before this split.
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<FrameJob>(2);
    let stop_capture = stop.clone();
    let cap_thread = {
        let mut cap = cap;
        std::thread::spawn(move || -> Result<()> {
            while !stop_capture.load(Ordering::Relaxed) {
                let timing = cap.capture_frame().context("capturing a frame")?;
                let Some(dmabuf) = cap.dmabuf(timing.buffer_index) else {
                    anyhow::bail!("capture returned a buffer with no dmabuf");
                };
                let plane = dmabuf
                    .planes
                    .first()
                    .context("the captured buffer has no planes")?;
                let job = FrameJob {
                    fd: plane
                        .fd
                        .try_clone()
                        .context("duplicating the captured buffer's fd")?,
                    offset: plane.offset,
                    stride: plane.stride,
                    pts_ns: timing.presentation_ns.unwrap_or(0),
                };
                if frame_tx.send(job).is_err() {
                    break;
                }
            }
            Ok(())
        })
    };

    // Reuse the evdi path's packetizer: it is what turns the Annex-B stream
    // into access units, extracts SPS/PPS for the tablet and numbers frames.
    let mut packetizer = H264AnnexBPacketizer::new(Codec::H264);
    let mut frames: u64 = 0;
    let mut last_report = std::time::Instant::now();
    let mut sent: u64 = 0;

    while let Ok(job) = frame_rx.recv() {
        encoder
            .push_frame(job.fd.as_fd(), job.offset, job.stride, job.pts_ns)
            .context("handing the frame to the encoder")?;

        while let Some(packet) = encoder.pull_packet(Duration::from_millis(0))? {
            for video_packet in packetizer.push(&packet.data) {
                let seq = video_packet.seq;
                if tx.receiver_count() > 0 {
                    let _ = tx.send(video_packet);
                }
                latency.on_encoded(seq);
                sent += 1;
            }
        }

        // The evdi path reports latency from its read loop; this loop is a
        // second producer of the same metric and has to do the same.
        latency.maybe_report();

        if let Some(config) = packetizer.codec_config() {
            if let Ok(mut guard) = codec_config.lock() {
                if guard.as_ref() != Some(&config) {
                    info!("codec config (SPS+PPS): {} bytes", config.len());
                    *guard = Some(config);
                }
            }
        }

        frames += 1;
        if last_report.elapsed() >= Duration::from_secs(10) {
            info!(
                "DMA-BUF stream: {} frames captured, {} packets sent, {:.1} fps",
                frames,
                sent,
                frames as f64 / last_report.elapsed().as_secs_f64()
            );
            frames = 0;
            sent = 0;
            last_report = std::time::Instant::now();
        }
    }

    encoder.stop();
    match cap_thread.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => anyhow::bail!("capture thread panicked"),
    }
}

/// One captured frame on its way to the encoder, with its own fd so it does
/// not borrow the capture pool across threads.
struct FrameJob {
    fd: std::os::fd::OwnedFd,
    offset: u32,
    stride: u32,
    pts_ns: u64,
}
