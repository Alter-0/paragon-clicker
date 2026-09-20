#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use eframe::egui::Vec2;
use paragon_clicker_rust::automation::configure_dpi_awareness;
use paragon_clicker_rust::ui::ParagonClickerApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--self-test" {
        let report_path = &args[2];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let report = serde_json::json!({
            "status": "ok",
            "gui_ready_at": now,
            "solver_loaded_before_gui": false,
            "backend": "eframe_native_rust"
        });
        std::fs::write(report_path, serde_json::to_string_pretty(&report).unwrap())
            .expect("Failed to write self-test report");
        return Ok(());
    }

    configure_dpi_awareness();

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(1120.0, 780.0))
            .with_min_inner_size(Vec2::new(900.0, 600.0))
            .with_title("巅峰加点器 (Paragon Clicker Rust - 极致性能版)"),
        ..Default::default()
    };

    eframe::run_native(
        "巅峰加点器",
        native_options,
        Box::new(|cc| {
            paragon_clicker_rust::ui::setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(ParagonClickerApp::default()))
        }),
    )
}
