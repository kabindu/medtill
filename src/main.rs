mod app;
mod models;
mod db;
mod scanner;
mod ui;
mod validate;

use app::MedTillApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_maximized(true),
        ..Default::default()
    };

    eframe::run_native(
        "MedTill",
        options,
        Box::new(|cc| {
            ui::theme::apply(&cc.egui_ctx);
            let app = MedTillApp::new().expect("failed to open medtill.sqlite");
            Ok(Box::new(app))
        }),
    )
}