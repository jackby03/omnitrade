//! Öffentlicher Abruf historischer Kerzen (`GET /api/v5/market/candles`, ohne Zugangsdaten).

use omnitrade_core::Candle;
use serde_json::Value;

use crate::binance::klines::get_text_with_headers;
use crate::credentials::Network;
use crate::error::ExchangeError;
use crate::okx::client::interval_code;

const BASE_URL: &str = "https://www.okx.com";
/// Obergrenze der Börse pro Abruf.
const MAX_LIMIT: usize = 300;

/// Lädt die letzten bestätigten Kerzen (älteste zuerst, höchstens `limit`).
///
/// Auf dem Demo (`Network::Testnet`) geht `x-simulated-trading: 1` mit: Spot und
/// normale Perps haben dort dieselben Kerzen, die XPERP-Kontrakte aber einen
/// eigenen Verfallscode (HYPE: Mainnet 310523, Demo 310801), der nur mit dem
/// Header existiert (sonst 51001).
///
/// # Errors
/// Fehler beim Abruf, bei nicht unterstütztem Intervall oder unlesbarer Antwort.
pub async fn fetch_klines(
    network: Network,
    symbol: &str,
    interval: &str,
    limit: usize,
) -> Result<Vec<Candle>, ExchangeError> {
    let bar = interval_code(interval).ok_or_else(|| {
        ExchangeError::InvalidRequest(format!(
            "Intervall '{interval}' wird von OKX nicht unterstützt"
        ))
    })?;
    let url = format!("{BASE_URL}/api/v5/market/candles");
    let limit = limit.clamp(1, MAX_LIMIT).to_string();
    let body = get_text_with_headers(
        &url,
        &[
            ("instId", symbol.to_uppercase().as_str()),
            ("bar", bar),
            ("limit", limit.as_str()),
        ],
        demo_headers(network),
    )
    .await?;
    parse_candles(&body)
}

/// Zusatz-Header je Netz: nur das Demo schickt `x-simulated-trading: 1`.
fn demo_headers(network: Network) -> &'static [(&'static str, &'static str)] {
    match network {
        Network::Testnet => &[("x-simulated-trading", "1")],
        Network::Mainnet => &[],
    }
}

/// Parst `{"code":"0","data":[[ts,o,h,l,c,vol,volCcy,volCcyQuote,confirm],..]}`
/// (neueste zuerst) in aufsteigende, bestätigte Kerzen.
fn parse_candles(body: &str) -> Result<Vec<Candle>, ExchangeError> {
    let parse_err = |reason: &str| ExchangeError::MessageParseFailed {
        raw: body.chars().take(200).collect(),
        reason: reason.to_string(),
    };
    let value: Value = serde_json::from_str(body).map_err(|e| parse_err(&e.to_string()))?;
    let code = value.get("code").and_then(Value::as_str).unwrap_or("");
    if code != "0" {
        let msg = value.get("msg").and_then(Value::as_str).unwrap_or("");
        return Err(ExchangeError::ApiError(format!("{code}: {msg}")));
    }
    let rows = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| parse_err("data fehlt"))?;
    let mut candles = Vec::new();
    for row in rows {
        let f = |i: usize| {
            row.get(i)
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<f64>().ok())
                .ok_or_else(|| parse_err("ungültiges Kerzenfeld"))
        };
        let ts = row
            .get(0)
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| parse_err("ungültiger Zeitstempel"))?;
        if row.get(8).and_then(Value::as_str) != Some("1") {
            continue; // offene Kerze
        }
        candles.push(Candle::new(ts, f(1)?, f(2)?, f(3)?, f(4)?, f(5)?));
    }
    candles.reverse();
    Ok(candles)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"code":"0","msg":"","data":[
        ["3000","1","2","0.5","1.5","10","0","0","0"],
        ["2000","1","2","0.5","1.6","11","0","0","1"],
        ["1000","1","2","0.5","1.7","12","0","0","1"]
    ]}"#;

    #[test]
    fn returns_confirmed_candles_oldest_first() {
        // Act
        let candles = parse_candles(SAMPLE).expect("valid");

        // Assert
        assert_eq!(candles.len(), 2, "open candle must be dropped");
        assert_eq!(candles[0].timestamp_ms, 1000);
        assert_eq!(candles[1].timestamp_ms, 2000);
        assert_eq!((candles[0].close, candles[0].volume), (1.7, 12.0));
    }

    #[test]
    fn only_demo_sends_simulated_trading_header() {
        assert_eq!(
            demo_headers(Network::Testnet),
            &[("x-simulated-trading", "1")]
        );
        assert!(demo_headers(Network::Mainnet).is_empty());
    }

    #[test]
    fn surfaces_api_errors_and_garbage() {
        match parse_candles(r#"{"code":"51001","msg":"Instrument ID does not exist","data":[]}"#) {
            Err(ExchangeError::ApiError(m)) => assert!(m.starts_with("51001")),
            other => panic!("expected ApiError, got {other:?}"),
        }
        assert!(parse_candles("kein json").is_err());
        assert!(
            parse_candles(r#"{"code":"0","data":[["x","1","1","1","1","1","0","0","1"]]}"#)
                .is_err()
        );
    }
}
