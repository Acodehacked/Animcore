mod app;
mod canvas;
mod panels;
mod sm_editor;
mod tools;

use app::AnimCoreApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnimCore Editor")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "AnimCore Editor",
        options,
        Box::new(|cc| Ok(Box::new(AnimCoreApp::new(cc)))),
    )
}
