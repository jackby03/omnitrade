//! Main application entry point for `omnitrade-ui`.

use std::sync::{Arc, RwLock};

use omnitrade_ui::{OmniTradeApp, UIState};

fn main() -> eframe::Result<()> {
    let state = Arc::new(RwLock::new(UIState::new()));
    let app = OmniTradeApp::new(state);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("omnitrade"),
        ..Default::default()
    };

    eframe::run_native("omnitrade", options, Box::new(|_cc| Ok(Box::new(app))))
}
