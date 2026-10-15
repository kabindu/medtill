//! Barcodes: the barcode box used on Checkout, Medicines and Stock, and the
//! webcam reader behind its "Camera" button.
//!
//! - A **USB barcode scanner** types the code and presses Enter, like a
//!   keyboard, so it needs no code at all: it just types into the box.
//! - **Camera** runs the webcam on its own thread with a live preview, and
//!   checks a frame every 150 ms with `rxing` (a pure-Rust ZXing port).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui;
use nokhwa::Camera;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};

use crate::ui::theme;

/// The webcam part of a barcode box. Off until "Camera" is clicked.
#[derive(Default)]
pub struct CameraScan {
    stream: Option<CameraStream>,
    camera_name: String,
    preview: Option<egui::TextureHandle>,
    message: Option<String>,
}

impl CameraScan {
    pub fn is_on(&self) -> bool {
        self.stream.is_some()
    }

    /// Finds the first camera and turns it on.
    pub fn start(&mut self) {
        self.stop();
        self.message = None;

        match list_cameras() {
            Ok(mut cameras) if !cameras.is_empty() => {
                let camera = cameras.remove(0);
                self.camera_name = camera.name;
                self.stream = Some(CameraStream::start(camera.index));
            }
            Ok(_) => {
                self.message = Some("No camera found. Plug in a USB camera, or use a USB barcode scanner.".to_string());
            }
            Err(e) => self.message = Some(e),
        }
    }

    /// Turns the camera off (dropping the stream stops its thread).
    pub fn stop(&mut self) {
        self.stream = None;
        self.preview = None;
    }
}

/// Draws a barcode text box with a Camera button (and the live camera
/// preview when it's on). `text` is the box's contents, owned by the screen.
/// Returns `Some(code)` on the frame a barcode was scanned — by a USB
/// scanner/typing + Enter, or by the camera.
///
/// `autofocus`: keep the cursor in this box whenever nothing else has it,
/// so a USB scanner's "typing" always lands here (used on Checkout).
pub fn barcode_box(
    ui: &mut egui::Ui,
    text: &mut String,
    camera: &mut CameraScan,
    id_salt: &str,
    hint: &'static str,
    autofocus: bool,
) -> Option<String> {
    let mut scanned = None;

    ui.horizontal(|ui| {
        let width = (ui.available_width() - 150.0).max(140.0);
        let response = ui.add(egui::TextEdit::singleline(text).id_salt(id_salt).hint_text(hint).desired_width(width));

        // A USB scanner (or the user) presses Enter after the code.
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let code = text.trim().to_string();
            if !code.is_empty() {
                scanned = Some(code);
            }
        }

        if autofocus && ui.memory(|m| m.focused().is_none()) {
            response.request_focus();
        }

        let label = if camera.is_on() { "Stop camera" } else { "Camera" };
        if ui.button(label).clicked() {
            if camera.is_on() {
                camera.stop();
            } else {
                camera.start();
            }
        }
    });

    if let Some(code) = show_camera(ui, camera) {
        *text = code.clone();
        scanned = Some(code);
    }

    scanned
}

/// Live preview + barcode pick-up. Returns a code the camera just read.
fn show_camera(ui: &mut egui::Ui, camera: &mut CameraScan) -> Option<String> {
    let Some(stream) = &camera.stream else {
        if let Some(message) = &camera.message {
            ui.colored_label(theme::STATUS_CRITICAL, message);
        }
        return None;
    };

    // Tell the camera thread this screen is still showing it.
    stream.heartbeat();

    if let Some(frame) = stream.take_frame() {
        let size = [frame.width as usize, frame.height as usize];
        let image = egui::ColorImage::from_rgb(size, &frame.rgb);
        if let Some(texture) = &mut camera.preview {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            camera.preview = Some(ui.ctx().load_texture("barcode_camera_preview", image, egui::TextureOptions::LINEAR));
        }
    }

    let code = stream.take_barcode();
    if let Some(error) = stream.error() {
        camera.message = Some(error);
    }
    let running = stream.is_running();

    ui.label(
        egui::RichText::new(format!(
            "{} — hold the barcode flat, about 15–25 cm from the camera, in good light.",
            camera.camera_name
        ))
        .color(theme::TEXT_SECONDARY),
    );

    let width = ui.available_width();
    match &camera.preview {
        Some(texture) => {
            ui.add(egui::Image::from_texture(texture).max_width(width).max_height(220.0));
        }
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Starting camera...");
            });
        }
    }

    if let Some(message) = &camera.message {
        ui.colored_label(theme::STATUS_CRITICAL, message);
    }

    if running {
        // Keep redrawing so the preview stays live.
        ui.ctx().request_repaint();
    } else {
        // The camera stopped (error, or this screen was hidden for a while).
        camera.stream = None;
        camera.preview = None;
    }

    code
}

// ---------------------------------------------------------------------------
// The camera thread
// ---------------------------------------------------------------------------

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

/// One picture as plain RGB bytes (3 bytes per pixel, row by row).
struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// A camera found on this computer.
struct FoundCamera {
    pub index: CameraIndex,
    pub name: String,
}

/// Lists the cameras Windows can see (USB webcams, built-in laptop cameras).
/// An empty list means no camera is connected.
fn list_cameras() -> Result<Vec<FoundCamera>, String> {
    let cameras = nokhwa::query(ApiBackend::Auto).map_err(|e| format!("Could not look for cameras: {e}"))?;
    Ok(cameras
        .into_iter()
        .map(|camera| FoundCamera { index: camera.index().clone(), name: camera.human_name() })
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
struct CameraStream {
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

        Self { shared, thread: Some(thread) }
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
        let idle = shared.last_seen.lock().map(|seen| seen.elapsed() > IDLE_SHUTDOWN).unwrap_or(true);
        if idle {
            break;
        }

        let decoded = camera.frame().and_then(|buffer| buffer.decode_image::<RgbFormat>());

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
                if !is_repeat && let Ok(mut slot) = shared.found_code.lock() {
                    *slot = Some(code.clone());
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
fn read_barcode(rgb: &[u8], width: u32, height: u32) -> Option<String> {
    let luma = rgb_to_luma(rgb);
    rxing::helpers::detect_in_luma(luma, width, height, None)
        .ok()
        .map(|result| result.getText().trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Converts RGB to grayscale (one byte per pixel), which is what barcode
/// readers work on. Integer weights approximate 0.30 R + 0.59 G + 0.11 B.
fn rgb_to_luma(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3).map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8).collect()
}
