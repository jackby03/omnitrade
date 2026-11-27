//! Request-Bausteine für OKX-Perpetuals (SWAP) im Modus **isolated** und **one-way** (`net_mode`).
//!
//! Die Werte sind bewusst fest verdrahtet: Abweichungen (Cross-Margin, Hedge-Modus,
//! mehr als 10x) sind nicht implementiert und werden abgelehnt.

use serde_json::{json, Value};

use crate::error::ExchangeError;
use crate::trading::OrderRequest;
use omnitrade_core::{OrderType, Side};

/// Harter Hebel-Deckel für SWAP. Die Instrumentenliste nennt bis 100x; das Konto
/// bekommt davon nicht automatisch alles.
pub const MAX_LEVERAGE: u32 = 10;
/// Einziger unterstützter Margin-Modus.
pub const MARGIN_MODE: &str = "isolated";
/// Einziger unterstützter Positionsmodus (one-way).
pub const POSITION_MODE: &str = "net_mode";

/// Erzeugt den Body für `/api/v5/trade/order` bei SWAP.
///
/// `contracts` ist bereits in Kontrakten umgerechnet und auf die Schrittweite gerundet.
#[must_use]
pub fn order_body(req: &OrderRequest, contracts_text: &str, px: Option<f64>) -> Value {
    let ord_type = match req.order_type {
        OrderType::Market => "market",
        _ => "limit",
    };
    let mut body = json!({
        "instId": req.symbol,
        "tdMode": MARGIN_MODE,
        "side": if req.side == Side::Buy { "buy" } else { "sell" },
        "ordType": ord_type,
        "sz": contracts_text,
    });
    if let Some(price) = px {
        body["px"] = json!(price.to_string());
    }
    body
}

/// Body für `/api/v5/account/set-leverage`. Lehnt Hebel außerhalb 1..=[`MAX_LEVERAGE`] ab.
///
/// # Errors
/// `InvalidRequest`, wenn `lever` außerhalb des erlaubten Bereichs liegt.
pub fn leverage_body(inst_id: &str, lever: u32) -> Result<Value, ExchangeError> {
    if lever == 0 || lever > MAX_LEVERAGE {
        return Err(ExchangeError::InvalidRequest(format!(
            "Hebel {lever}x außerhalb 1..={MAX_LEVERAGE}"
        )));
    }
    Ok(json!({
        "instId": inst_id,
        "lever": lever.to_string(),
        "mgnMode": MARGIN_MODE,
    }))
}

/// Prüft die Antwort von `/api/v5/account/config`: nur `net_mode` ist erlaubt.
///
/// # Errors
/// `AccountError`, wenn das Konto nicht im one-way-Modus ist oder die Antwort fehlt.
pub fn check_position_mode(data: &[Value]) -> Result<(), ExchangeError> {
    let mode = data
        .first()
        .and_then(|d| d.get("posMode"))
        .and_then(Value::as_str)
        .ok_or_else(|| ExchangeError::AccountError("Antwort ohne posMode".to_string()))?;
    if mode == POSITION_MODE {
        Ok(())
    } else {
        Err(ExchangeError::AccountError(format!(
            "Positionsmodus ist '{mode}', erforderlich ist '{POSITION_MODE}' (one-way)"
        )))
    }
}

/// Anzahl Nachkommastellen einer Schrittweite (`0.01` → 2).
#[must_use]
pub fn decimals_of(step: f64) -> usize {
    (0..=8)
        .find(|&d| {
            let scaled = step * 10f64.powi(d as i32);
            (scaled - scaled.round()).abs() < 1e-9
        })
        .unwrap_or(8)
}

/// Formatiert Kontrakte mit der Nachkommazahl der Schrittweite, z. B. `1.49` bei `0.01`.
#[must_use]
pub fn format_contracts(contracts: f64, lot_sz: f64) -> String {
    format!("{contracts:.*}", decimals_of(lot_sz))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market_buy(qty: f64) -> OrderRequest {
        OrderRequest::market("BTC-USDT-SWAP".to_string(), Side::Buy, qty)
    }

    #[test]
    fn market_order_body_is_isolated_without_target_currency() {
        // Arrange + Act
        let body = order_body(&market_buy(0.05), "5.00", None);
        // Assert
        assert_eq!(body["tdMode"], "isolated");
        assert_eq!(body["ordType"], "market");
        assert_eq!(body["sz"], "5.00");
        assert!(body.get("tgtCcy").is_none());
        assert!(body.get("posSide").is_none());
    }

    #[test]
    fn limit_order_body_carries_price() {
        let req = OrderRequest::limit("BTC-USDT-SWAP".to_string(), Side::Sell, 0.02, 50_000.5);
        let body = order_body(&req, "2.00", Some(50_000.5));
        assert_eq!(body["side"], "sell");
        assert_eq!(body["ordType"], "limit");
        assert_eq!(body["px"], "50000.5");
    }

    #[test]
    fn leverage_is_capped_at_ten() {
        assert!(leverage_body("BTC-USDT-SWAP", MAX_LEVERAGE).is_ok());
        assert!(leverage_body("BTC-USDT-SWAP", MAX_LEVERAGE + 1).is_err());
        assert!(leverage_body("BTC-USDT-SWAP", 0).is_err());
    }

    #[test]
    fn leverage_body_is_isolated() {
        let body = leverage_body("BTC-USDT-SWAP", 3).expect("valid leverage");
        assert_eq!(body["lever"], "3");
        assert_eq!(body["mgnMode"], "isolated");
    }

    #[test]
    fn only_net_mode_passes_the_position_check() {
        let net = vec![json!({"posMode": "net_mode"})];
        let hedge = vec![json!({"posMode": "long_short_mode"})];
        assert!(check_position_mode(&net).is_ok());
        assert!(check_position_mode(&hedge).is_err());
        assert!(check_position_mode(&[]).is_err());
    }

    #[test]
    fn contracts_are_formatted_to_lot_precision() {
        assert_eq!(format_contracts(1.49, 0.01), "1.49");
        assert_eq!(format_contracts(5.0, 0.01), "5.00");
        assert_eq!(format_contracts(3.0, 1.0), "3");
        assert_eq!(decimals_of(0.001), 3);
    }
}
