//! Candlestick chart widget for egui with viewport culling and auto-scaling Y axis.

use std::ops::Range;

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use omnitrade_core::Candle;

/// Interactive candlestick chart widget with viewport culling.
#[derive(Debug, Clone)]
pub struct CandleChartWidget {
    /// Candlestick data points.
    pub candles: Vec<Candle>,
    /// Range of currently visible candle indices.
    pub visible_range: Range<usize>,
    /// Width of a single candle body in pixels.
    pub candle_width: f32,
    /// Horizontal spacing between candles in pixels.
    pub spacing: f32,
    /// Horizontal scroll offset in pixels.
    pub scroll_offset: f32,
}

impl CandleChartWidget {
    /// Creates a new `CandleChartWidget` with default parameters.
    pub fn new(candles: Vec<Candle>) -> Self {
        Self {
            candles,
            visible_range: 0..0,
            candle_width: 8.0,
            spacing: 2.0,
            scroll_offset: 0.0,
        }
    }

    /// Sets or replaces the candle dataset.
    pub fn set_candles(&mut self, candles: Vec<Candle>) {
        self.candles = candles;
    }

    /// Pans the chart horizontally by `delta` pixels.
    pub fn handle_scroll(&mut self, delta: f32) {
        self.scroll_offset = (self.scroll_offset + delta).max(0.0);
    }

    /// Calculates the range of candle indices visible within the given viewport width.
    pub fn compute_visible_range(
        total_candles: usize,
        scroll_offset: f32,
        candle_width: f32,
        spacing: f32,
        viewport_width: f32,
    ) -> Range<usize> {
        if total_candles == 0 || viewport_width <= 0.0 {
            return 0..0;
        }

        let stride = (candle_width + spacing).max(0.1);
        let start_idx = ((scroll_offset / stride).floor() as usize).min(total_candles);
        let visible_count = (viewport_width / stride).ceil() as usize + 1;
        let end_idx = (start_idx + visible_count).min(total_candles);

        start_idx..end_idx
    }

    /// Determines the display color for a candle (Green for bullish, Red for bearish).
    pub fn candle_color(candle: &Candle) -> Color32 {
        if candle.is_bullish() {
            Color32::from_rgb(38, 166, 154)
        } else {
            Color32::from_rgb(239, 83, 80)
        }
    }

    /// Renders the chart within the provided egui UI context.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let available_size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(available_size, Sense::drag());

        if response.dragged() {
            self.handle_scroll(-response.drag_delta().x);
        }

        self.visible_range = Self::compute_visible_range(
            self.candles.len(),
            self.scroll_offset,
            self.candle_width,
            self.spacing,
            rect.width(),
        );

        if self.visible_range.is_empty() || self.visible_range.start >= self.candles.len() {
            return;
        }

        let visible_candles = &self.candles[self.visible_range.clone()];
        let (min_price, max_price) = visible_candles.iter().fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(min_p, max_p), candle| (min_p.min(candle.low), max_p.max(candle.high)),
        );

        let price_span = if (max_price - min_price).abs() < f64::EPSILON {
            1.0
        } else {
            max_price - min_price
        };

        let painter = ui.painter_at(rect);

        // Draw horizontal grid lines and price labels
        let grid_lines = 4;
        for i in 0..=grid_lines {
            let t = i as f32 / grid_lines as f32;
            let y = rect.bottom() - t * rect.height();
            let price = min_price + t as f64 * price_span;

            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(1.0, Color32::from_gray(50)),
            );

            painter.text(
                Pos2::new(rect.left() + 4.0, y - 6.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.2}", price),
                egui::FontId::proportional(10.0),
                Color32::from_gray(180),
            );
        }

        let stride = self.candle_width + self.spacing;

        // Draw candles
        for (idx_offset, candle) in visible_candles.iter().enumerate() {
            let candle_idx = self.visible_range.start + idx_offset;
            let x_center = rect.left() + (candle_idx as f32 * stride) - self.scroll_offset
                + (self.candle_width / 2.0);

            if x_center < rect.left() || x_center > rect.right() {
                continue;
            }

            let price_to_y = |p: f64| -> f32 {
                let norm = ((p - min_price) / price_span) as f32;
                rect.bottom() - norm * rect.height()
            };

            let high_y = price_to_y(candle.high);
            let low_y = price_to_y(candle.low);
            let open_y = price_to_y(candle.open);
            let close_y = price_to_y(candle.close);

            let color = Self::candle_color(candle);

            // Wick
            painter.line_segment(
                [Pos2::new(x_center, high_y), Pos2::new(x_center, low_y)],
                Stroke::new(1.0, color),
            );

            // Body
            let top_y = open_y.min(close_y);
            let bottom_y = open_y.max(close_y);
            let body_height = (bottom_y - top_y).max(1.0);

            let body_rect = Rect::from_min_size(
                Pos2::new(x_center - self.candle_width / 2.0, top_y),
                Vec2::new(self.candle_width, body_height),
            );

            painter.rect_filled(body_rect, 0.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candle_color_bullish_and_bearish() {
        // Arrange
        let bullish = Candle::new(1000, 100.0, 110.0, 90.0, 105.0, 10.0);
        let bearish = Candle::new(2000, 105.0, 110.0, 90.0, 100.0, 10.0);
        let doji = Candle::new(3000, 100.0, 105.0, 95.0, 100.0, 5.0);

        // Act
        let bull_color = CandleChartWidget::candle_color(&bullish);
        let bear_color = CandleChartWidget::candle_color(&bearish);
        let doji_color = CandleChartWidget::candle_color(&doji);

        // Assert
        assert_eq!(bull_color, Color32::from_rgb(38, 166, 154));
        assert_eq!(bear_color, Color32::from_rgb(239, 83, 80));
        assert_eq!(doji_color, Color32::from_rgb(38, 166, 154));
    }

    #[test]
    fn test_compute_visible_range() {
        // Arrange
        let total = 100;
        let candle_width = 8.0;
        let spacing = 2.0;
        let viewport_width = 50.0;

        // Act & Assert (scroll_offset = 0)
        let range1 = CandleChartWidget::compute_visible_range(
            total,
            0.0,
            candle_width,
            spacing,
            viewport_width,
        );
        assert_eq!(range1, 0..6);

        // Act & Assert (scroll_offset = 25.0 -> start_idx = 2)
        let range2 = CandleChartWidget::compute_visible_range(
            total,
            25.0,
            candle_width,
            spacing,
            viewport_width,
        );
        assert_eq!(range2, 2..8);

        // Act & Assert (empty candles)
        let range_empty =
            CandleChartWidget::compute_visible_range(0, 0.0, candle_width, spacing, viewport_width);
        assert_eq!(range_empty, 0..0);
    }

    #[test]
    fn test_handle_scroll_panning() {
        // Arrange
        let mut widget = CandleChartWidget::new(vec![]);
        assert_eq!(widget.scroll_offset, 0.0);

        // Act (pan forward)
        widget.handle_scroll(15.0);

        // Assert
        assert_eq!(widget.scroll_offset, 15.0);

        // Act (pan backward past zero)
        widget.handle_scroll(-30.0);

        // Assert
        assert_eq!(widget.scroll_offset, 0.0);
    }
}
