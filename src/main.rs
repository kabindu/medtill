// A release build opens no black console window behind MedTill.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod db;
mod medicines;
mod models;
mod pdf;
mod scanner;
mod storage;
mod sync;
mod ui;
mod validate;

use std::sync::Arc;

fn main() -> eframe::Result<()> {
    storage::start_dir(); // remembered before moving into the data folder
    let opened = storage::prepare().and_then(|moved| {
        let conn = db::open(&storage::db_path().to_string_lossy()).map_err(String::from)?;
        Ok((conn, moved))
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_maximized(opened.is_ok())
            .with_icon(Arc::new(icon())),
        ..Default::default()
    };
    eframe::run_native(
        "MedTill",
        options,
        Box::new(move |cc| {
            ui::theme::apply(&cc.egui_ctx);
            Ok(match opened {
                Ok((conn, moved)) => Box::new(app::MedTillApp::new(conn, moved)),
                Err(problem) => Box::new(StartupError(problem)),
            })
        }),
    )
}

/// Shown instead of the till when the data can't be opened, so the person
/// sees what is wrong instead of the program just vanishing.
struct StartupError(String);

impl eframe::App for StartupError {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(40.0);
            ui.heading("MedTill could not start");
            ui.add_space(8.0);
            ui.label(&self.0);
            ui.add_space(8.0);
            ui::theme::muted(
                ui,
                format!(
                    "Your data folder is {}. Close any other MedTill window and try again. \
                     If it keeps happening, restore the latest backup from Documents\\MedTill backups.",
                    storage::data_dir().display()
                ),
            );
        });
    }
}

/// The window and taskbar icon, drawn here so no image file is needed: a
/// blue rounded square with a white cross.
fn icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (cx, cy) = (x.min(SIZE - 1 - x), y.min(SIZE - 1 - y)); // distance from the nearest edges
            let corner = cx < 10 && cy < 10 && (10 - cx).pow(2) + (10 - cy).pow(2) > 100;
            let cross = (x.abs_diff(SIZE / 2) < 7 && (14..50).contains(&y))
                || (y.abs_diff(SIZE / 2) < 7 && (14..50).contains(&x));
            rgba.extend_from_slice(match (corner, cross) {
                (true, _) => &[0, 0, 0, 0],
                (false, true) => &[255, 255, 255, 255],
                (false, false) => &[0x2a, 0x78, 0xd6, 255],
            });
        }
    }
    egui::IconData { rgba, width: SIZE, height: SIZE }
}
