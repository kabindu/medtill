//! Camera + barcode reading.
//!
//! MedTill reads barcodes two ways:
//! 1. A **USB barcode scanner** (recommended). It behaves like a keyboard:
//!    it types the digits and presses Enter, so it needs no code here at
//!    all — the barcode boxes in `ui/scan.rs` just receive the "typing".
//! 2. A **webcam**, handled in this file: the camera runs on its own thread,
//!    shows a live preview, and every few frames the picture is checked for
//!    a barcode with `rxing` (a pure-Rust port of the ZXing barcode reader).
//!
//! Nothing in this file touches the screen or the database.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;

/// How often a camera frame is checked for a barcode. Decoding takes a few
/// tens of milliseconds, so not every frame is checked.
const DECODE_EVERY: Duration = Duration::from_millis(150);

/// The same barcode isn't reported again until it has been out of view for
/// this long — otherwise holding one box in front of the camera would add
/// it to the cart over and over.
const SAME_CODE_PAUSE: Duration = Duration::from_millis(2500);

/// If the screen showing the camera hasn't been drawn for this long (the
/// user switched to another tab), the camera turns itself off.
const IDLE_SHUTDOWN: Duration = Duration::from_millis(1500);

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// One picture as plain RGB bytes (3 bytes per pixel, row by row).
///
/// It owns its pixels (`Vec<u8>`) because it outlives the camera buffer it
/// was decoded from and crosses from the camera thread to the screen.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

/// A camera found on this computer.
pub struct FoundCamera {
    pub index: CameraIndex,
    pub name: String,
}

/// Lists the cameras Windows can see (USB webcams, built-in laptop cameras).
/// An empty list means no camera is connected.
pub fn list_cameras() -> Result<Vec<FoundCamera>, String> {
    let cameras =
        nokhwa::query(ApiBackend::Auto).map_err(|e| format!("Could not look for cameras: {e}"))?;
    Ok(cameras
        .into_iter()
        .map(|camera| FoundCamera {
            index: camera.index().clone(),
            name: camera.human_name(),
        })
        .collect())
}

/// Everything the camera thread and the screen share. Each field has its
/// own lock (or atomic) so neither side ever waits long on the other.
struct Shared {
    latest_frame: Mutex<Option<Frame>>,
    found_code: Mutex<Option<String>>,
    error: Mutex<Option<String>>,
    last_seen: Mutex<Instant>,
    stop: AtomicBool,
    running: AtomicBool,
}

/// A running camera that reads barcodes. Dropping it turns the camera off.
pub struct CameraStream {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl CameraStream {
    pub fn start(index: CameraIndex) -> Self {
        let shared = Arc::new(Shared {
            latest_frame: Mutex::new(None),
            found_code: Mutex::new(None),
            error: Mutex::new(None),
            last_seen: Mutex::new(Instant::now()),
            stop: AtomicBool::new(false),
            running: AtomicBool::new(true),
        });

        let thread = {
            let shared = Arc::clone(&shared);
            // The `Camera` is created *inside* the thread, so it never has to
            // be sent between threads.
            std::thread::spawn(move || {
                camera_loop(index, &shared);
                shared.running.store(false, Ordering::Relaxed);
            })
        };

        Self {
            shared,
            thread: Some(thread),
        }
    }

    /// The newest preview frame since the last call, if any.
    pub fn take_frame(&self) -> Option<Frame> {
        self.shared.latest_frame.lock().ok().and_then(|mut slot| slot.take())
    }

    /// A barcode the camera has just read, if any (each one is returned once).
    pub fn take_barcode(&self) -> Option<String> {
        self.shared.found_code.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Set if the camera failed to open or stopped working.
    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().ok().and_then(|slot| slot.clone())
    }

    /// False once the camera thread has ended (error, idle shutdown, or stop).
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    /// Call every time the camera's panel is drawn. If this stops being
    /// called (the user moved to another screen), the camera turns off.
    pub fn heartbeat(&self) {
        if let Ok(mut last_seen) = self.shared.last_seen.lock() {
            *last_seen = Instant::now();
        }
    }
}

impl Drop for CameraStream {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn camera_loop(index: CameraIndex, shared: &Shared) {
    let report = |message: String| {
        if let Ok(mut slot) = shared.error.lock() {
            *slot = Some(message);
        }
    };

    let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
    let mut camera = match Camera::new(index, format) {
        Ok(camera) => camera,
        Err(e) => {
            report(format!("Could not open the camera: {e}"));
            return;
        }
    };

    if let Err(e) = camera.open_stream() {
        report(format!("Could not start the camera: {e}"));
        return;
    }

    let mut last_decode: Option<Instant> = None;
    let mut last_code: Option<(String, Instant)> = None;

    while !shared.stop.load(Ordering::Relaxed) {
        let idle = shared
            .last_seen
            .lock()
            .map(|seen| seen.elapsed() > IDLE_SHUTDOWN)
            .unwrap_or(true);
        if idle {
            break;
        }

        let decoded = camera
            .frame()
            .and_then(|buffer| buffer.decode_image::<RgbFormat>());

        let image = match decoded {
            Ok(image) => image,
            Err(e) => {
                report(format!("The camera stopped: {e}"));
                break;
            }
        };
        let (width, height) = image.dimensions();
        let rgb = image.into_raw();

        // Every DECODE_EVERY, look for a barcode in this frame.
        if last_decode.is_none_or(|at| at.elapsed() >= DECODE_EVERY) {
            last_decode = Some(Instant::now());
            if let Some(code) = read_barcode(&rgb, width, height) {
                let is_repeat = matches!(
                    &last_code,
                    Some((previous, seen_at)) if *previous == code && seen_at.elapsed() < SAME_CODE_PAUSE
                );
                if !is_repeat {
                    if let Ok(mut slot) = shared.found_code.lock() {
                        *slot = Some(code.clone());
                    }
                }
                // Refresh the timestamp while the same code stays in view.
                last_code = Some((code, Instant::now()));
            }
        }

        if let Ok(mut slot) = shared.latest_frame.lock() {
            *slot = Some(Frame { width, height, rgb });
        }
    }

    let _ = camera.stop_stream();
}

/// Looks for a barcode (EAN-13, Code 128, QR, DataMatrix, ...) in an RGB
/// picture. Returns its text, or `None` if there isn't a readable one.
pub fn read_barcode(rgb: &[u8], width: u32, height: u32) -> Option<String> {
    let luma = rgb_to_luma(rgb);
    rxing::helpers::detect_in_luma(luma, width, height, None)
        .ok()
        .map(|result| result.getText().trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Converts RGB to grayscale (one byte per pixel), which is what barcode
/// readers work on. Integer weights approximate 0.30 R + 0.59 G + 0.11 B.
fn rgb_to_luma(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3)
        .map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8)
        .collect()
}