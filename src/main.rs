mod app;
mod models;
mod db;
mod ui;
mod validate;

use app::MedTillApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MedTill",
        options,
        Box::new(|_cc| {
            let app = MedTillApp::new().expect("failed to open medtill.sqlite");
            Ok(Box::new(app))
        }),
    )
}