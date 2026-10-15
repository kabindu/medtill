mod app;
mod db;
mod models;
mod ui;
mod validate;

fn main() -> eframe::Result<()> {
    let conn = db::open("medtill.sqlite").unwrap_or_else(|e| panic!("Could not open medtill.sqlite: {e}"));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1600.0, 900.0]).with_maximized(true),
        ..Default::default()
    };

    eframe::run_native(
        "MedTill",
        options,
        Box::new(move |cc| {
            ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::MedTillApp::new(conn)))
        }),
    )
}
