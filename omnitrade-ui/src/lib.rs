//! UI state container, widgets, docking app, and passive channel subscriber for omnitrade-ui.

pub mod account_feed;
pub mod app;
pub mod feed;
pub mod kill;
pub mod state;
pub mod status_feed;
pub mod widgets;

pub use app::{OmniTradeApp, PanelKind};
pub use state::UIState;
pub use widgets::chart::CandleChartWidget;
pub use widgets::depth::DepthWidget;
