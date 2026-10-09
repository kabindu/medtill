//! The barcode box used on every screen that needs one (Inventory,
//! Checkout, Log a delivery).
//!
//! It's an ordinary text box plus a "Camera" button:
//! - A **USB barcode scanner** types the code into the box and presses
//!   Enter, exactly like a keyboard — so it works with no extra setup.
//! - The code can also be typed by hand (then press Enter).
//! - **Camera** turns on the webcam; when it sees a barcode it fills the box
//!   by itself.
//!
//! Each screen keeps its own `CameraScan` in its state and gets back
//! `Some(code)` from `barcode_box` whenever a barcode was scanned.

use eframe::egui;

use crate::scanner::{self, CameraStream};
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

        match scanner::list_cameras() {
            Ok(mut cameras) if !cameras.is_empty() => {
                let camera = cameras.remove(0);
                self.camera_name = camera.name;
                self.stream = Some(CameraStream::start(camera.index));
            }
            Ok(_) => {
                self.message = Some(
                    "No camera found. Plug in a USB camera, or use a USB barcode scanner.".to_string(),
                );
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
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .id_salt(id_salt)
                .hint_text(hint)
                .desired_width(width),
        );

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
            camera.preview = Some(ui.ctx().load_texture(
                "barcode_camera_preview",
                image,
                egui::TextureOptions::LINEAR,
            ));
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