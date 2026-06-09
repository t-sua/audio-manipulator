#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod audio;
mod config;
mod decoder;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("Audio Manipulator"),
        ..Default::default()
    };
    eframe::run_native(
        "Audio Manipulator",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::new()))),
    )
}
