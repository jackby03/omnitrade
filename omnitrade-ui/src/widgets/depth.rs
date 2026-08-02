//! Order book depth widget implementation.

use egui::{Color32, Rect, Sense, Ui, Vec2};
use omnitrade_engine::L2OrderBook;

/// Order book depth visualization widget displaying ask and bid levels.
#[derive(Debug, Clone)]
pub struct DepthWidget {
    /// Maximum number of price levels to render for bids/asks.
    pub max_levels: usize,
    /// Whether to display the spread row between asks and bids.
    pub show_spread: bool,
}

impl DepthWidget {
    /// Constructs a new `DepthWidget` with the specified maximum levels.
    pub fn new(max_levels: usize) -> Self {
        Self {
            max_levels,
            show_spread: true,
        }
    }

    /// Calculates the bid-ask spread percentage for an order book.
    pub fn spread_percentage(book: &L2OrderBook) -> Option<f64> {
        spread_percentage(book)
    }

    /// Renders the depth widget in the provided egui UI context.
    pub fn show(&self, ui: &mut Ui, book: Option<&L2OrderBook>) {
        let book = match book {
            Some(b) => b,
            None => {
                ui.label("Orderbook empty");
                return;
            }
        };

        let ask_levels: Vec<(f64, f64)> = book.asks().take(self.max_levels).collect();
        let bid_levels: Vec<(f64, f64)> = book.bids().take(self.max_levels).collect();

        if ask_levels.is_empty() && bid_levels.is_empty() {
            ui.label("Orderbook empty");
            return;
        }

        let max_vol = ask_levels
            .iter()
            .chain(bid_levels.iter())
            .map(|(_, q)| *q)
            .fold(0.0f64, f64::max);

        let max_vol = if max_vol > 0.0 { max_vol } else { 1.0 };

        ui.vertical(|ui| {
            // Header row
            ui.horizontal(|ui| {
                ui.columns(3, |cols| {
                    cols[0].label("Price");
                    cols[1].label("Qty");
                    cols[2].label("Cum Qty");
                });
            });
            ui.separator();

            // Render Ask levels (sorted ascending price)
            let mut cum_ask = 0.0;
            for (price, qty) in &ask_levels {
                cum_ask += qty;
                render_level_row(
                    ui,
                    *price,
                    *qty,
                    cum_ask,
                    max_vol,
                    Color32::from_rgba_unmultiplied(239, 83, 80, 40),
                );
            }

            // Render Spread row
            if self.show_spread {
                ui.separator();
                let spread_str = match (book.spread(), spread_percentage(book)) {
                    (Some(spread), Some(pct)) => format!("Spread: {:.2} ({:.2}%)", spread, pct),
                    _ => "Spread: N/A".to_string(),
                };
                ui.label(spread_str);
                ui.separator();
            }

            // Render Bid levels (sorted descending price)
            let mut cum_bid = 0.0;
            for (price, qty) in &bid_levels {
                cum_bid += qty;
                render_level_row(
                    ui,
                    *price,
                    *qty,
                    cum_bid,
                    max_vol,
                    Color32::from_rgba_unmultiplied(38, 166, 154, 40),
                );
            }
        });
    }
}

impl Default for DepthWidget {
    fn default() -> Self {
        Self::new(20)
    }
}

/// Helper function to render a single depth price level row with background volume bar.
fn render_level_row(
    ui: &mut Ui,
    price: f64,
    qty: f64,
    cum_qty: f64,
    max_vol: f64,
    bar_color: Color32,
) {
    let row_height = 18.0;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), row_height), Sense::hover());

    if ui.is_rect_visible(rect) {
        let ratio = (qty / max_vol).clamp(0.0, 1.0);
        let bar_width = rect.width() * ratio as f32;
        let bar_rect = Rect::from_min_size(rect.min, Vec2::new(bar_width, rect.height()));
        ui.painter().rect_filled(bar_rect, 0.0, bar_color);

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.columns(3, |cols| {
                cols[0].label(format!("{:.2}", price));
                cols[1].label(format!("{:.4}", qty));
                cols[2].label(format!("{:.4}", cum_qty));
            });
        });
    }
}

/// Calculates the bid-ask spread percentage: `(best_ask - best_bid) / mid_price * 100.0`.
pub fn spread_percentage(book: &L2OrderBook) -> Option<f64> {
    let (bid, _) = book.best_bid()?;
    let (ask, _) = book.best_ask()?;
    let mid = book.mid_price()?;
    if mid == 0.0 {
        return None;
    }
    Some((ask - bid) / mid * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_order_book_rendering() {
        // Arrange
        let widget = DepthWidget::default();
        let empty_book = L2OrderBook::new("BTCUSDT");

        // Act & Assert
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                widget.show(ui, None);
                widget.show(ui, Some(&empty_book));
            });
        });

        assert_eq!(empty_book.spread(), None);
        assert_eq!(spread_percentage(&empty_book), None);
    }

    #[test]
    fn test_spread_percentage_calculation() {
        // Arrange
        let mut book = L2OrderBook::new("BTCUSDT");
        book.apply_delta(&[(100.0, 1.0)], &[(110.0, 1.0)])
            .expect("delta update should succeed in test");

        // Act
        let pct = spread_percentage(&book);

        // Assert
        let expected = (110.0 - 100.0) / 105.0 * 100.0;
        assert!(pct.is_some());
        let pct_val = pct.expect("percentage should be Some");
        assert!((pct_val - expected).abs() < 1e-6);
    }

    #[test]
    fn test_max_levels_truncation() {
        // Arrange
        let mut book = L2OrderBook::new("BTCUSDT");
        let bids: Vec<(f64, f64)> = (1..=10).map(|i| (100.0 - i as f64, 1.0)).collect();
        let asks: Vec<(f64, f64)> = (1..=10).map(|i| (100.0 + i as f64, 1.0)).collect();
        book.apply_delta(&bids, &asks)
            .expect("delta update should succeed in test");
        let widget = DepthWidget::new(3);

        // Act
        let ask_levels: Vec<_> = book.asks().take(widget.max_levels).collect();
        let bid_levels: Vec<_> = book.bids().take(widget.max_levels).collect();

        // Assert
        assert_eq!(ask_levels.len(), 3);
        assert_eq!(bid_levels.len(), 3);
        assert_eq!(widget.max_levels, 3);
    }

    #[test]
    fn test_depth_widget_default_and_new() {
        // Arrange & Act
        let default_widget = DepthWidget::default();
        let custom_widget = DepthWidget::new(15);

        // Assert
        assert_eq!(default_widget.max_levels, 20);
        assert!(default_widget.show_spread);
        assert_eq!(custom_widget.max_levels, 15);
        assert!(custom_widget.show_spread);
    }
}
