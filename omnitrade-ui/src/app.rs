//! Main application layout with `egui_tiles` docking and central workspace panels.

use std::sync::{Arc, RwLock};

use std::path::PathBuf;

use omnitrade_core::{Order, Position};
use omnitrade_engine::L2OrderBook;

use crate::state::UIState;
use crate::status_feed;
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
    /// Snapshot of open positions, sorted by symbol.
    pub positions: &'a [Position],
    /// Snapshot of the console log lines (oldest first).
    pub log: &'a [String],
    /// Account balances `(asset, free, locked)`.
    pub balances: &'a [(String, f64, f64)],
    /// Open orders reported by the exchange.
    pub orders: &'a [Order],
    /// Lines for the positions panel (status summary from `--status-file`).
    pub status_summary: &'a [String],
    /// Recent orders from the status file, newest first.
    pub status_orders: &'a [String],
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
            PanelKind::Positions => show_positions(
                ui,
                self.positions,
                self.balances,
                self.orders,
                self.status_summary,
                self.status_orders,
            ),
            PanelKind::Console => show_console(ui, self.log),
        }
        egui_tiles::UiResponse::None
    }
}

fn show_positions(
    ui: &mut egui::Ui,
    positions: &[Position],
    balances: &[(String, f64, f64)],
    orders: &[Order],
    status_summary: &[String],
    status_orders: &[String],
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.strong("Positionen");
            if positions.is_empty() {
                ui.label("Keine offenen Positionen");
            } else {
                egui::Grid::new("positions_grid")
                    .striped(true)
                    .show(ui, |ui| {
                        for h in [
                            "Symbol",
                            "Seite",
                            "Menge",
                            "Einstand",
                            "Unrealisiert",
                            "Realisiert",
                        ] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for p in positions {
                            ui.label(&p.symbol);
                            ui.label(format!("{:?}", p.side));
                            ui.label(format!("{:.6}", p.quantity));
                            ui.label(format!("{:.2}", p.entry_price));
                            ui.label(format!("{:+.2}", p.unrealized_pnl));
                            ui.label(format!("{:+.2}", p.realized_pnl));
                            ui.end_row();
                        }
                    });
            }
            ui.separator();
            ui.strong("Offene Orders");
            if orders.is_empty() {
                ui.label("Keine offenen Orders");
            } else {
                egui::Grid::new("orders_grid").striped(true).show(ui, |ui| {
                    for h in ["Symbol", "Seite", "Typ", "Preis", "Menge", "Gefüllt"] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for o in orders {
                        ui.label(&o.symbol);
                        ui.label(format!("{:?}", o.side));
                        ui.label(format!("{:?}", o.order_type));
                        ui.label(format!("{:.2}", o.price));
                        ui.label(format!("{:.6}", o.quantity));
                        ui.label(format!("{:.6}", o.filled_quantity));
                        ui.end_row();
                    }
                });
            }
            ui.separator();
            ui.strong("Bestände");
            if balances.is_empty() {
                ui.label("Keine Kontodaten (Zugänge fehlen oder noch nicht geladen)");
            } else {
                egui::Grid::new("balances_grid")
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["Asset", "Frei", "Gesperrt"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for (asset, free, locked) in balances {
                            ui.label(asset);
                            ui.label(format!("{free:.8}"));
                            ui.label(format!("{locked:.8}"));
                            ui.end_row();
                        }
                    });
            }
            ui.separator();
            ui.strong("Live-Status (Statusdatei)");
            if status_summary.is_empty() {
                ui.weak("keine Statusdatei");
            } else {
                for line in status_summary {
                    ui.monospace(line);
                }
            }
            for line in status_orders {
                ui.monospace(line);
            }
        });
}

fn show_console(ui: &mut egui::Ui, log: &[String]) {
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for line in log {
                ui.monospace(line);
            }
        });
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
    /// Symbol shown in the chart and order book panels.
    pub symbol: String,
    /// Name of the exchange the data comes from (status bar label).
    pub venue: String,
    /// Path of the kill-switch flag file shared with `omnitrade-cli kill`.
    pub kill_path: PathBuf,
}

impl OmniTradeApp {
    /// Creates a new `OmniTradeApp` for `BTCUSDT` with the default layout.
    pub fn new(state: Arc<RwLock<UIState>>) -> Self {
        Self::with_symbol(state, "BTCUSDT")
    }

    /// Creates a new `OmniTradeApp` for the given symbol.
    ///
    /// Layout: chart and order book on top, positions and console below.
    pub fn with_symbol(state: Arc<RwLock<UIState>>, symbol: impl Into<String>) -> Self {
        let mut tiles = egui_tiles::Tiles::default();
        let chart_pane = tiles.insert_pane(PanelKind::Chart);
        let book_pane = tiles.insert_pane(PanelKind::OrderBook);
        let positions_pane = tiles.insert_pane(PanelKind::Positions);
        let console_pane = tiles.insert_pane(PanelKind::Console);
        let top = tiles.insert_horizontal_tile(vec![chart_pane, book_pane]);
        let bottom = tiles.insert_horizontal_tile(vec![positions_pane, console_pane]);
        let root = tiles.insert_vertical_tile(vec![top, bottom]);
        let tree = egui_tiles::Tree::new("omnitrade_tile_tree", root, tiles);

        Self {
            state,
            tree,
            chart: CandleChartWidget::new(Vec::new()),
            depth: DepthWidget::new(8),
            symbol: symbol.into(),
            venue: String::new(),
            kill_path: crate::kill::default_path(),
        }
    }
}

impl OmniTradeApp {
    /// Kill-switch button (sets the shared flag file) and, once set, a reset button.
    /// Returns the response of the visible button.
    ///
    /// The running live loop notices the flag, cancels the symbol's open orders and
    /// stops; it does not sell holdings.
    fn kill_controls(&mut self, ui: &mut egui::Ui) -> egui::Response {
        if crate::kill::is_engaged(&self.kill_path) {
            ui.colored_label(egui::Color32::LIGHT_RED, "NOT-AUS GESETZT");
            let response = ui.button("Zurücksetzen");
            if response.clicked() {
                self.report_kill(crate::kill::clear(&self.kill_path), "Not-Aus aufgehoben");
            }
            response
        } else {
            let response = ui.add(egui::Button::new("NOT-AUS").fill(egui::Color32::DARK_RED));
            if response.clicked() {
                self.report_kill(
                    crate::kill::engage(&self.kill_path),
                    "Not-Aus gesetzt: Live-Schleifen stornieren offene Orders und stoppen",
                );
            }
            response
        }
    }

    fn report_kill(&self, result: std::io::Result<()>, ok_message: &str) {
        let line = match result {
            Ok(()) => ok_message.to_string(),
            Err(e) => format!("Not-Aus-Datei nicht schreibbar: {e}"),
        };
        self.state
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .push_log(line);
    }
}

impl eframe::App for OmniTradeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (current_orderbook, mut positions, log, connected, balance, pnl, balances, orders) = {
            let guard = self
                .state
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.chart
                .set_candles(guard.latest_candles(&self.symbol, 100).to_vec());
            (
                guard.orderbook.get(&self.symbol).cloned(),
                guard.positions.values().cloned().collect::<Vec<_>>(),
                guard.log.iter().cloned().collect::<Vec<_>>(),
                guard.is_connected(),
                guard.account_balance,
                guard.total_pnl,
                guard.balances.clone(),
                guard.open_orders.clone(),
            )
        };
        positions.sort_by(|a, b| a.symbol.cmp(&b.symbol));

        egui::TopBottomPanel::top("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if !self.venue.is_empty() {
                    ui.label(&self.venue);
                }
                ui.strong(&self.symbol);
                ui.separator();
                if connected {
                    ui.colored_label(egui::Color32::LIGHT_GREEN, "verbunden");
                } else {
                    ui.colored_label(egui::Color32::LIGHT_RED, "getrennt");
                }
                ui.separator();
                ui.label(format!("Guthaben {balance:.2}"));
                ui.label(format!("PnL {pnl:+.2}"));
                ui.separator();
                let _ = self.kill_controls(ui);
            });
        });
        let (status_summary, status_orders) = {
            let guard = self
                .state
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut summary = Vec::new();
            let mut orders = Vec::new();
            if let Some(doc) = &guard.status {
                summary = status_feed::summary_lines(doc);
                orders = status_feed::order_lines(doc);
            }
            if let Some(err) = &guard.feed_error {
                summary.insert(0, format!("feed: {err}"));
            }
            (summary, orders)
        };

        let mut behavior = OmniTradeAppBehavior {
            chart: &mut self.chart,
            depth: &self.depth,
            orderbook: current_orderbook.as_ref(),
            positions: &positions,
            log: &log,
            balances: &balances,
            status_summary: &status_summary,
            status_orders: &status_orders,
            orders: &orders,
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
        assert_eq!(app.depth.max_levels, 8);
        assert_eq!(app.symbol, "BTCUSDT");
        assert!(app.tree.root().is_some());
    }

    #[test]
    fn test_layout_contains_all_four_panels() {
        // Arrange & Act
        let app = OmniTradeApp::with_symbol(Arc::new(RwLock::new(UIState::new())), "ETHUSDT");

        // Assert
        assert_eq!(app.symbol, "ETHUSDT");
        for kind in [
            PanelKind::Chart,
            PanelKind::OrderBook,
            PanelKind::Positions,
            PanelKind::Console,
        ] {
            assert!(
                app.tree
                    .tiles
                    .tiles()
                    .any(|t| matches!(t, egui_tiles::Tile::Pane(k) if *k == kind)),
                "panel {kind:?} missing"
            );
        }
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

    /// Runs one frame of the kill controls and returns the button response.
    fn frame(
        ctx: &egui::Context,
        app: &mut OmniTradeApp,
        events: Vec<egui::Event>,
    ) -> egui::Response {
        let mut response = None;
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                response = Some(app.kill_controls(ui));
            });
        });
        response.expect("button response")
    }

    fn click(ctx: &egui::Context, app: &mut OmniTradeApp) {
        let pos = frame(ctx, app, vec![]).rect.center();
        frame(ctx, app, vec![egui::Event::PointerMoved(pos)]);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(ctx, app, vec![button(true)]);
        frame(ctx, app, vec![button(false)]);
    }

    #[test]
    fn test_kill_button_engages_and_reset_button_clears_flag() {
        // Arrange
        let path = std::env::temp_dir().join(format!("omnitrade-ui-btn-{}", std::process::id()));
        let _ = crate::kill::clear(&path);
        let mut app = OmniTradeApp::new(Arc::new(RwLock::new(UIState::new())));
        app.kill_path = path.clone();
        let ctx = egui::Context::default();

        // Act & Assert: click NOT-AUS
        click(&ctx, &mut app);
        assert!(
            crate::kill::is_engaged(&path),
            "flag must be set after click"
        );
        assert!(app
            .state
            .read()
            .expect("lock")
            .log
            .iter()
            .any(|l| l.contains("Not-Aus gesetzt")));

        // Act & Assert: click Zurücksetzen
        click(&ctx, &mut app);
        assert!(
            !crate::kill::is_engaged(&path),
            "flag must be cleared after reset"
        );
    }
}
