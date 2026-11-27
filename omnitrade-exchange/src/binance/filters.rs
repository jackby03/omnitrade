//! Handelsregeln eines Symbols (`GET /api/v3/exchangeInfo`, öffentlich): Tick-Size,
//! Step-Size und Mindestmengen. Daraus leitet das Grid seine Rundung ab, statt sie
//! für BTCUSDT fest anzunehmen.

use serde_json::Value;

use crate::binance::klines::{base_url, get_text};
use crate::credentials::Network;
use crate::error::ExchangeError;

/// Handelsregeln eines Symbols, auf das Nötigste reduziert.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolFilters {
    /// Nachkommastellen des Preises (Tick-Size = 10^-n).
    pub price_decimals: u32,
    /// Nachkommastellen der Menge (Step-Size = 10^-n).
    pub qty_decimals: u32,
    /// Kleinste erlaubte Menge.
    pub min_qty: f64,
    /// Kleinster erlaubter Orderwert in der Quote-Währung (0, wenn nicht gesetzt).
    pub min_notional: f64,
}

/// Lädt die Handelsregeln eines Symbols.
///
/// # Errors
/// Fehler beim Abruf, unbekanntes Symbol, unlesbare Antwort oder eine Tick-/Step-Size,
/// die keine Zehnerpotenz ist (dafür ist die Dezimalrundung nicht korrekt).
pub async fn fetch_symbol_filters(
    network: Network,
    symbol: &str,
) -> Result<SymbolFilters, ExchangeError> {
    let url = format!("{}/api/v3/exchangeInfo", base_url(network));
    let upper = symbol.to_uppercase();
    let body = get_text(&url, &[("symbol", upper.as_str())]).await?;
    parse_filters(&body, &upper)
}

/// Anzahl Nachkommastellen einer Zehnerpotenz-Größe (`"0.01000000"` → 2, `"1.00000000"` → 0).
fn decimals_of_power_of_ten(raw: &str, what: &str) -> Result<u32, ExchangeError> {
    let value: f64 = raw.parse().map_err(|_| ExchangeError::MessageParseFailed {
        raw: raw.to_string(),
        reason: format!("{what} ist keine Zahl"),
    })?;
    if !(value > 0.0 && value <= 1.0) {
        return Err(ExchangeError::InvalidRequest(format!(
            "{what} {raw} wird nicht unterstützt"
        )));
    }
    let decimals = (-value.log10()).round() as u32;
    if (value * 10f64.powi(decimals as i32) - 1.0).abs() > 1e-9 {
        return Err(ExchangeError::InvalidRequest(format!(
            "{what} {raw} ist keine Zehnerpotenz; Dezimalrundung wäre falsch"
        )));
    }
    Ok(decimals)
}

fn parse_filters(body: &str, symbol: &str) -> Result<SymbolFilters, ExchangeError> {
    let parse_err = |reason: &str| ExchangeError::MessageParseFailed {
        raw: body.chars().take(200).collect(),
        reason: reason.to_string(),
    };
    let value: Value = serde_json::from_str(body).map_err(|e| parse_err(&e.to_string()))?;
    let info = value
        .get("symbols")
        .and_then(Value::as_array)
        .and_then(|s| {
            s.iter()
                .find(|s| s.get("symbol").and_then(Value::as_str) == Some(symbol))
        })
        .ok_or_else(|| ExchangeError::InvalidRequest(format!("Symbol {symbol} unbekannt")))?;
    let filters = info
        .get("filters")
        .and_then(Value::as_array)
        .ok_or_else(|| parse_err("filters fehlt"))?;
    let field = |filter_type: &str, key: &str| -> Option<&str> {
        filters
            .iter()
            .find(|f| f.get("filterType").and_then(Value::as_str) == Some(filter_type))?
            .get(key)?
            .as_str()
    };
    let tick = field("PRICE_FILTER", "tickSize").ok_or_else(|| parse_err("PRICE_FILTER fehlt"))?;
    let step = field("LOT_SIZE", "stepSize").ok_or_else(|| parse_err("LOT_SIZE fehlt"))?;
    let min_qty = field("LOT_SIZE", "minQty")
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| parse_err("minQty fehlt"))?;
    let min_notional = field("NOTIONAL", "minNotional")
        .or_else(|| field("MIN_NOTIONAL", "minNotional"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    Ok(SymbolFilters {
        price_decimals: decimals_of_power_of_ten(tick, "Tick-Size")?,
        qty_decimals: decimals_of_power_of_ten(step, "Step-Size")?,
        min_qty,
        min_notional,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(tick: &str, step: &str) -> String {
        format!(
            r#"{{"symbols":[{{"symbol":"BTCUSDT","filters":[
                {{"filterType":"PRICE_FILTER","tickSize":"{tick}"}},
                {{"filterType":"LOT_SIZE","minQty":"0.00001000","stepSize":"{step}"}},
                {{"filterType":"NOTIONAL","minNotional":"5.00000000"}}
            ]}}]}}"#
        )
    }

    #[test]
    fn derives_decimals_and_minimums() {
        // Act
        let f = parse_filters(&body("0.01000000", "0.00001000"), "BTCUSDT").expect("valid");

        // Assert
        assert_eq!(
            f,
            SymbolFilters {
                price_decimals: 2,
                qty_decimals: 5,
                min_qty: 0.00001,
                min_notional: 5.0
            }
        );
    }

    #[test]
    fn handles_other_powers_of_ten() {
        let f = parse_filters(&body("0.00000100", "1.00000000"), "BTCUSDT").expect("valid");
        assert_eq!((f.price_decimals, f.qty_decimals), (6, 0));
        let f = parse_filters(&body("1.00000000", "0.10000000"), "BTCUSDT").expect("valid");
        assert_eq!((f.price_decimals, f.qty_decimals), (0, 1));
    }

    #[test]
    fn rejects_non_power_of_ten_ticks() {
        // Eine Tick-Size von 0.05 verlangt Vielfache von 0.05, nicht 2 Nachkommastellen.
        assert!(matches!(
            parse_filters(&body("0.05000000", "0.00001000"), "BTCUSDT"),
            Err(ExchangeError::InvalidRequest(_))
        ));
        assert!(parse_filters(&body("0.01000000", "0.50000000"), "BTCUSDT").is_err());
        assert!(parse_filters(&body("10.00000000", "0.00001000"), "BTCUSDT").is_err());
        assert!(parse_filters(&body("0.00000000", "0.00001000"), "BTCUSDT").is_err());
    }

    #[test]
    fn unknown_symbol_and_garbage_are_errors() {
        assert!(parse_filters(&body("0.01", "0.001"), "ETHUSDT").is_err());
        assert!(parse_filters("kein json", "BTCUSDT").is_err());
        assert!(parse_filters(
            r#"{"symbols":[{"symbol":"BTCUSDT","filters":[]}]}"#,
            "BTCUSDT"
        )
        .is_err());
    }

    #[test]
    fn missing_notional_filter_means_zero() {
        let raw = r#"{"symbols":[{"symbol":"BTCUSDT","filters":[
            {"filterType":"PRICE_FILTER","tickSize":"0.01"},
            {"filterType":"LOT_SIZE","minQty":"0.001","stepSize":"0.001"}]}]}"#;
        assert_eq!(
            parse_filters(raw, "BTCUSDT").expect("valid").min_notional,
            0.0
        );
    }
}
