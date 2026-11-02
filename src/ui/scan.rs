//! The find box (a person, or a USB scanner, types a code or name and
//! presses Enter) and the "Photograph the box" panel, which reads a
//! medicine's details with the camera (the reading itself is in `scanner`).
//!
//! Every scan saved teaches the reader (`learn`, see `scanner::train`): the
//! box's words are remembered for that medicine, a wrongly picked line's
//! words are remembered as not-a-name, the name-picking weights move a
//! little, and the scan is kept as a training example in
//! `training/scans.jsonl` beside the database.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::medicines::SLOGANS;
use crate::models::Product;
use crate::scanner::{self, Camera, Found, Frame, Reader, Taught};
use crate::ui::theme::{self, Notice};

/// Photos of one box: the front (name), a side (batch and expiry), the back.
const MAX_PHOTOS: usize = 3;

/// Where what the reader was taught is kept (the settings table): its
/// feature weights, and words that are not names (space-separated).
const WEIGHTS_KEY: &str = "scan_weights";
const NOT_NAMES_KEY: &str = "scan_not_names";

/// Saved scans, one JSON line each; started afresh (the old one kept as
/// `scans.old.jsonl`) past about 5 MB, a few thousand scans.
const TRAINING_FILE: &str = "training/scans.jsonl";
const TRAINING_OLD: &str = "training/scans.old.jsonl";
const TRAINING_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// A text box that returns what was typed when Enter is pressed.
/// `autofocus` keeps the cursor here whenever nothing else has it, so a
/// USB scanner's typing always lands in it (used on Checkout).
pub fn find_box(ui: &mut egui::Ui, text: &mut String, id: &str, hint: &str, autofocus: bool) -> Option<String> {
    let edit = egui::TextEdit::singleline(text).id_salt(id).hint_text(hint).desired_width(f32::INFINITY);
    let response = ui.add(edit);
    if autofocus && ui.memory(|m| m.focused().is_none()) {
        response.request_focus();
    }
    let entered = text.trim().to_string();
    (theme::enter_pressed(&response) && !entered.is_empty()).then_some(entered)
}

/// The photo panel. Closed, it holds nothing; open, the camera and the text
/// reader run on their own threads until it closes again.
#[derive(Default)]
pub struct BoxScan {
    open: bool,
    camera: Option<Camera>,
    reader: Option<Reader>,
    preview: Option<egui::TextureHandle>,
    latest: Option<Frame>,
    photos: Vec<Frame>,
    reading: bool,
    found: Option<(Found, Duration)>,
    message: Notice,
}

impl BoxScan {
    fn start(&mut self) {
        // The reader starts loading now, while the box is being photographed.
        *self = BoxScan { open: true, reader: Some(Reader::start()), ..Default::default() };
        match Camera::start() {
            Ok(camera) => self.camera = Some(camera),
            Err(e) => self.message = Notice::error(e),
        }
    }
}

/// The "Photograph the box" button and, once pressed, the live camera, the
/// photos, and what was read. Returns the details on "Use these details".
pub fn box_scan(ui: &mut egui::Ui, conn: &Connection, st: &mut BoxScan, products: &[Product]) -> Option<Found> {
    if !st.open {
        let button = ui.button("Photograph the box").on_hover_text("Reads the name, category, batch and expiry");
        if button.clicked() {
            st.start();
        }
        return None;
    }
    let (mut used, mut again, mut close) = (None, false, false);
    theme::panel(ui, |ui| {
        theme::subheading(ui, "Photograph the box");
        if let Some((found, took)) = &mut st.found {
            show_found(ui, found, products);
            theme::muted(ui, format!("Read in {:.1} s. Check the details before saving.", took.as_secs_f32()));
            theme::actions(ui, |ui| {
                if theme::primary_button(ui, "Use these details").clicked() {
                    used = Some(found.clone());
                }
                again = ui.button("Take again").clicked();
                close = theme::quiet_button(ui, "Close").clicked();
            });
        } else if st.reading {
            reading(ui, conn, st, products);
        } else {
            camera(ui, st);
        }
        st.message.show(ui);
    });
    if again {
        st.start();
    } else if used.is_some() || close {
        *st = BoxScan::default(); // camera and reader off, memory freed
    }
    used
}

/// The live picture with "Take photo" and "Read the box".
fn camera(ui: &mut egui::Ui, st: &mut BoxScan) {
    // A reader that couldn't load (model files missing) says so once.
    if let Some(Err(e)) = st.reader.as_ref().and_then(Reader::poll) {
        st.message = Notice::error(e);
        st.reader = None;
    }
    if let Some(camera) = &st.camera {
        camera.heartbeat();
        if let Some(frame) = camera.take_frame() {
            let image = egui::ColorImage::from_rgb([frame.width as usize, frame.height as usize], &frame.rgb);
            match &mut st.preview {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => st.preview = Some(ui.ctx().load_texture("box_photo", image, egui::TextureOptions::LINEAR)),
            }
            st.latest = Some(frame);
        }
        if let Some(e) = camera.error() {
            st.message = Notice::error(e);
        }
        if camera.is_running() {
            ui.ctx().request_repaint();
        } else {
            st.camera = None;
        }
    }
    match &st.preview {
        Some(texture) => {
            ui.add(egui::Image::from_texture(texture).max_width(ui.available_width()).max_height(240.0));
        }
        None if st.camera.is_some() => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Starting the camera...");
            });
        }
        None => {}
    }
    let camera = st.camera.as_ref().map_or(String::new(), |c| format!("{}: ", c.name));
    theme::muted(
        ui,
        format!(
            "{camera}hold the box 15–25 cm away, in good light. Photograph the front (the name), \
             then the side with the batch and expiry."
        ),
    );
    theme::actions(ui, |ui| {
        let can_take = st.latest.is_some() && st.photos.len() < MAX_PHOTOS;
        let take = format!("Take photo ({}/{MAX_PHOTOS})", st.photos.len());
        if ui.add_enabled_ui(can_take, |ui| ui.button(take)).inner.clicked()
            && let Some(frame) = st.latest.clone()
        {
            st.photos.push(frame);
        }
        let can_read = !st.photos.is_empty() && st.reader.is_some();
        if ui.add_enabled_ui(can_read, |ui| theme::primary_button(ui, "Read the box")).inner.clicked()
            && let Some(reader) = &st.reader
        {
            reader.read(std::mem::take(&mut st.photos));
            (st.camera, st.preview, st.latest, st.reading) = (None, None, None, true);
            st.message = Notice::None;
        }
        if theme::quiet_button(ui, "Cancel").clicked() {
            *st = BoxScan::default();
        }
    });
}

/// Waits for the reader, then turns its text into details.
fn reading(ui: &mut egui::Ui, conn: &Connection, st: &mut BoxScan, products: &[Product]) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label("Reading the box...");
    });
    ui.ctx().request_repaint_after(Duration::from_millis(100));
    let Some(result) = st.reader.as_ref().and_then(Reader::poll) else { return };
    st.reading = false;
    match result {
        Ok((lines, _)) if lines.is_empty() => {
            st.start();
            st.message = Notice::error("No writing found. Try again closer to the camera, in better light.");
        }
        Ok((lines, took)) => {
            // Without its memory (a database error) the reader still works, untaught.
            let taught = taught(conn).unwrap_or_default();
            let found = scanner::read_box(&lines, products, &taught, chrono::Local::now().date_naive());
            st.found = Some((found, took));
        }
        Err(e) => st.message = Notice::error(e),
    }
}

fn show_found(ui: &mut egui::Ui, found: &mut Found, products: &[Product]) {
    let known = found.product_id.as_ref().and_then(|id| products.iter().find(|p| &p.id == id));
    let or_dash = |text: &str| if text.is_empty() { "—".to_string() } else { text.to_string() };
    theme::form(ui, "box_found", |ui| {
        let rows = [
            ("Medicine", or_dash(&found.name)),
            ("Category", or_dash(&found.category)),
            ("Batch", or_dash(&found.batch)),
            ("Expiry", found.expiry.map_or("—".to_string(), |d| d.format("%-d %b %Y").to_string())),
        ];
        for (label, value) in rows {
            ui.label(egui::RichText::new(label).color(theme::TEXT_SECONDARY));
            ui.label(egui::RichText::new(value).strong());
            ui.end_row();
        }
    });
    match known {
        Some(p) => ui.colored_label(theme::STATUS_GOOD, format!("● Already listed as {}", p.label())),
        None => ui.colored_label(theme::ACCENT_STRONG, "● New medicine"),
    };
    // Other lines that could be the name: one tap swaps it in (and its
    // category, if the medicine list knows it). A new medicine only.
    if known.is_none() && !found.alternatives.is_empty() {
        ui.add_space(4.0);
        let items: Vec<(&str, bool)> = found.alternatives.iter().map(|a| (a.as_str(), true)).collect();
        if let Some(i) = theme::chip_row(ui, "Not the right name? Tap the one on the box", &items) {
            let picked = std::mem::replace(&mut found.alternatives[i], std::mem::take(&mut found.name));
            let text: Vec<&str> = found.lines.iter().map(|l| l.text.as_str()).collect();
            if let Some(category) = scanner::list_category(&[&picked, &text.join(" \n ")], products) {
                found.category = category;
            }
            found.name = picked;
        }
    }
}

/// Learns from a scan that was saved as `saved_name` (`saved_category`)
/// for medicine `product_id`. Learning never stops a save: a failure here
/// is returned for a notice, the medicine is saved either way.
pub fn learn(
    conn: &Connection,
    found: &Found,
    product_id: &str,
    saved_name: &str,
    saved_category: &str,
) -> Result<(), String> {
    let mut taught = taught(conn)?;
    if scanner::train(&mut taught, found, saved_name) {
        db::put_setting(conn, WEIGHTS_KEY, &scanner::weights_to_text(&taught.weights))?;
        db::put_setting(conn, NOT_NAMES_KEY, &taught.not_names.join(" "))?;
    }
    // The box's words, remembered for this medicine (not slogans or labels).
    let words: Vec<String> = found
        .words
        .iter()
        .filter(|w| !SLOGANS.contains(&w.as_str()) && !taught.not_names.contains(w))
        .cloned()
        .collect();
    db::learn_box_words(conn, product_id, &words)?;
    let when = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    keep_example(&scanner::training_example(found, saved_name, saved_category, &when))
        .map_err(|e| format!("the training example wasn't kept ({e})"))
}

/// What this shop has taught the reader so far.
fn taught(conn: &Connection) -> Result<Taught, String> {
    Ok(Taught {
        words: db::box_words(conn)?,
        weights: scanner::weights_from_text(&db::setting(conn, WEIGHTS_KEY)?),
        not_names: db::setting(conn, NOT_NAMES_KEY)?.split_whitespace().map(String::from).collect(),
    })
}

fn keep_example(line: &str) -> std::io::Result<()> {
    fs::create_dir_all(Path::new(TRAINING_FILE).parent().unwrap_or(Path::new(".")))?;
    if fs::metadata(TRAINING_FILE).is_ok_and(|m| m.len() > TRAINING_MAX_BYTES) {
        fs::rename(TRAINING_FILE, TRAINING_OLD)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(TRAINING_FILE)?;
    writeln!(file, "{line}")
}
