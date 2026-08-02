//! Main application layout with `egui_tiles` docking and central workspace panels.

use std::sync::{Arc, RwLock};

use omnitrade_engine::L2OrderBook;

use crate::state::UIState;
use crate::widgets::chart::CandleChartWidget;
use crate::widgets::depth::DepthWidget;

/// Identifies the type of content rendered in a workspace tile pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    /// Candlestick price chart.
    Chart,
    /// Level-2 order book depth visualization.
    OrderBook,
    /// Active positions summary.
    Positions,
    /// System and trade execution logs.
    Console,
}

impl PanelKind {
    /// Returns the human-readable display title for the panel tab.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Chart => "Chart",
            Self::OrderBook => "Order Book",
            Self::Positions => "Positions",
            Self::Console => "Console",
        }
    }
}

/// Custom `egui_tiles::Behavior` implementation for rendering tile contents.
pub struct OmniTradeAppBehavior<'a> {
    /// Mutable reference to the chart widget.
    pub chart: &'a mut CandleChartWidget,
    /// Reference to the order book depth widget.
    pub depth: &'a DepthWidget,
    /// Optional snapshot of the active symbol's L2 order book.
    pub orderbook: Option<&'a L2OrderBook>,
}

impl<'a> egui_tiles::Behavior<PanelKind> for OmniTradeAppBehavior<'a> {
    fn tab_title_for_pane(&mut self, pane: &PanelKind) -> egui::WidgetText {
        pane.title().into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut PanelKind,
    ) -> egui_tiles::UiResponse {
        match pane {
            PanelKind::Chart => {
                self.chart.show(ui);
            }
            PanelKind::OrderBook => {
                self.depth.show(ui, self.orderbook);
            }
            PanelKind::Positions => {
                ui.label("Positions Panel (Placeholder)");
            }
            PanelKind::Console => {
                ui.label("Console Panel (Placeholder)");
            }
        }
        egui_tiles::UiResponse::None
    }
}

/// Main `omnitrade` application state holding UI tree and sub-widgets.
pub struct OmniTradeApp {
    /// Shared reference to the thread-safe passive `UIState`.
    pub state: Arc<RwLock<UIState>>,
    /// Tile layout tree managing dockable panels.
    pub tree: egui_tiles::Tree<PanelKind>,
    /// Candlestick chart widget.
    pub chart: CandleChartWidget,
    /// Order book depth widget.
    pub depth: DepthWidget,
}

impl OmniTradeApp {
    /// Creates a new `OmniTradeApp` initialized with default tiles (`Chart` & `OrderBook`).
    pub fn new(state: Arc<RwLock<UIState>>) -> Self {
        let mut tiles = egui_tiles::Tiles::default();
        let chart_pane = tiles.insert_pane(PanelKind::Chart);
        let book_pane = tiles.insert_pane(PanelKind::OrderBook);
        let root = tiles.insert_horizontal_tile(vec![chart_pane, book_pane]);
        let tree = egui_tiles::Tree::new("omnitrade_tile_tree", root, tiles);

        Self {
            state,
            tree,
            chart: CandleChartWidget::new(Vec::new()),
            depth: DepthWidget::new(15),
        }
    }
}

impl eframe::App for OmniTradeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        const ACTIVE_SYMBOL: &str = "BTCUSDT";

        let mut current_orderbook = None;

        let guard = self
            .state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let candles = guard.latest_candles(ACTIVE_SYMBOL, 100).to_vec();
        self.chart.set_candles(candles);
        if let Some(book) = guard.orderbook.get(ACTIVE_SYMBOL) {
            current_orderbook = Some(book.clone());
        }

        let mut behavior = OmniTradeAppBehavior {
            chart: &mut self.chart,
            depth: &self.depth,
            orderbook: current_orderbook.as_ref(),
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            self.tree.ui(&mut behavior, ui);
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnitrade_core::Candle;

    #[test]
    fn test_panel_kind_titles() {
        // Arrange & Act & Assert
        assert_eq!(PanelKind::Chart.title(), "Chart");
        assert_eq!(PanelKind::OrderBook.title(), "Order Book");
        assert_eq!(PanelKind::Positions.title(), "Positions");
        assert_eq!(PanelKind::Console.title(), "Console");
    }

    #[test]
    fn test_omnitrade_app_default_initialization() {
        // Arrange
        let state = Arc::new(RwLock::new(UIState::new()));

        // Act
        let app = OmniTradeApp::new(state);

        // Assert
        assert_eq!(app.chart.candles.len(), 0);
        assert_eq!(app.depth.max_levels, 15);
        assert!(app.tree.root().is_some());
    }

    #[test]
    fn test_uistate_snapshot_reading() {
        // Arrange
        let state = Arc::new(RwLock::new(UIState::new()));
        {
            let mut guard = state.write().expect("Failed to acquire write lock in test");
            guard.update_candle("BTCUSDT", Candle::new(100, 100.0, 105.0, 99.0, 104.0, 10.0));
        }
        let mut app = OmniTradeApp::new(state);

        // Act
        let guard = app.state.read().unwrap_or_else(|p| p.into_inner());
        let candles = guard.latest_candles("BTCUSDT", 10).to_vec();
        app.chart.set_candles(candles);

        // Assert
        assert_eq!(app.chart.candles.len(), 1);
        assert_eq!(app.chart.candles[0].close, 104.0);
    }
}
