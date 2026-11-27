//! Trading trait abstraction for exchange order placement and management.
//! Exchange implementations (Binance, Kraken, OKX) conform to this interface.

use crate::error::ExchangeError;
use omnitrade_core::{Order, OrderType, Side};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Request for placing a new order.
#[derive(Debug, Clone)]
pub struct OrderRequest {
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: f64,
    pub price: Option<f64>,
}

impl OrderRequest {
    /// Create a new market order request.
    pub fn market(symbol: String, side: Side, quantity: f64) -> Self {
        Self {
            symbol,
            side,
            order_type: OrderType::Market,
            quantity,
            price: None,
        }
    }

    /// Create a new limit order request.
    pub fn limit(symbol: String, side: Side, quantity: f64, price: f64) -> Self {
        Self {
            symbol,
            side,
            order_type: OrderType::Limit,
            quantity,
            price: Some(price),
        }
    }
}

/// Ausfuehrungsdaten einer Order, soweit die Boerse sie liefert.
///
/// `fee` ist die Gebuehr mit Vorzeichen wie von der Boerse (Kosten negativ).
/// `fee_ccy` ist die Waehrung, in der sie abgezogen wurde.
#[derive(Debug, Clone, PartialEq)]
pub struct FillDetails {
    pub filled: f64,
    pub avg_price: f64,
    pub fee: f64,
    pub fee_ccy: String,
}

/// Account balance snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalance {
    pub free: f64,
    pub locked: f64,
}

impl AccountBalance {
    /// Total available + locked balance.
    pub fn total(&self) -> f64 {
        self.free + self.locked
    }
}

/// Account information including all balances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub balances: std::collections::HashMap<String, AccountBalance>,
}

impl fmt::Display for OrderRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} {:?} {} {}",
            self.side, self.order_type, self.quantity, self.symbol
        )
    }
}

/// Core trading interface for order placement and cancellation.
/// Implementations must be Send + Sync for use in async contexts.
#[async_trait::async_trait]
pub trait ExchangeTrading: Send + Sync {
    /// Place a new order on the exchange.
    ///
    /// # Errors
    /// Returns `ExchangeError` if the request is invalid or exchange responds with error.
    async fn place_order(&self, req: &OrderRequest) -> Result<Order, ExchangeError>;

    /// Cancel an existing order by ID.
    ///
    /// # Errors
    /// Returns `ExchangeError` if order not found or cancellation fails.
    async fn cancel_order(&self, order_id: &str, symbol: &str) -> Result<(), ExchangeError>;

    /// Retrieve all open orders for the account.
    ///
    /// # Errors
    /// Returns `ExchangeError` if API call fails.
    async fn get_open_orders(&self) -> Result<Vec<Order>, ExchangeError>;

    /// Fetch current account balances.
    ///
    /// # Errors
    /// Returns `ExchangeError` if API call fails.
    async fn get_balance(&self) -> Result<Account, ExchangeError>;

    /// Optionally fetch a single order by ID.
    async fn get_order(
        &self,
        order_id: &str,
        _symbol: &str,
    ) -> Result<Option<Order>, ExchangeError> {
        // Default: not all exchanges support single-order fetching
        let orders = self.get_open_orders().await?;
        let order_id_num: u64 = order_id
            .parse()
            .map_err(|_| ExchangeError::InvalidRequest("Invalid order ID format".to_string()))?;
        Ok(orders.into_iter().find(|o| o.id == order_id_num))
    }

    /// Ausfuehrungsdaten (Durchschnittspreis, Gebuehr) einer Order.
    ///
    /// Standard: unbekannt (None). Boersen, die diese Daten liefern, ueberschreiben es.
    async fn fill_details(
        &self,
        _order_id: &str,
        _symbol: &str,
    ) -> Result<Option<FillDetails>, ExchangeError> {
        Ok(None)
    }

    /// Offene Position im Symbol. Spot hat keine Positionen: der Default liefert null.
    /// Perpetual-Adapter überschreiben dies mit den echten Werten.
    async fn position_snapshot(&self, _symbol: &str) -> Result<PositionSnapshot, ExchangeError> {
        Ok(PositionSnapshot::default())
    }
}

/// Position und unrealisierter PnL eines Symbols, wie sie die Risikoprüfung braucht.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PositionSnapshot {
    /// `true` für Perpetuals. Dann ersetzt `base_qty` den Spot-Bestand als Exposure.
    pub is_perpetual: bool,
    /// Vorzeichenbehaftete Menge in Basiswährung (positiv = long).
    pub base_qty: f64,
    /// Unrealisierter PnL in der Quote-/Settlement-Währung.
    pub unrealized_pnl: f64,
    /// Liquidationspreis der Position, sofern offen. Vom Exchange berechnet.
    pub liq_price: Option<f64>,
    /// Markpreis laut Exchange, sofern offen.
    pub mark_price: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_request_market() {
        let req = OrderRequest::market("BTCUSDT".to_string(), Side::Buy, 1.0);
        assert_eq!(req.symbol, "BTCUSDT");
        assert_eq!(req.side, Side::Buy);
        assert_eq!(req.order_type, OrderType::Market);
        assert_eq!(req.price, None);
    }

    #[test]
    fn test_order_request_limit() {
        let req = OrderRequest::limit("ETHUSDT".to_string(), Side::Sell, 2.5, 2500.0);
        assert_eq!(req.symbol, "ETHUSDT");
        assert_eq!(req.side, Side::Sell);
        assert_eq!(req.order_type, OrderType::Limit);
        assert_eq!(req.price, Some(2500.0));
    }

    #[test]
    fn test_order_request_display() {
        let req = OrderRequest::limit("BTCUSDT".to_string(), Side::Buy, 0.5, 45000.0);
        let display = format!("{}", req);
        assert!(display.contains("Buy"));
        assert!(display.contains("Limit"));
        assert!(display.contains("0.5"));
        assert!(display.contains("BTCUSDT"));
    }

    #[test]
    fn test_account_balance_total() {
        let balance = AccountBalance {
            free: 100.0,
            locked: 50.0,
        };
        assert_eq!(balance.total(), 150.0);
    }

    #[test]
    fn test_account_balance_serialization() {
        let balance = AccountBalance {
            free: 123.45,
            locked: 67.89,
        };
        let json = serde_json::to_string(&balance).expect("serialization failed");
        let deserialized: AccountBalance =
            serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(deserialized.free, balance.free);
        assert_eq!(deserialized.locked, balance.locked);
    }
}
