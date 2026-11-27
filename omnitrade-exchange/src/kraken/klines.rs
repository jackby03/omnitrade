//! Öffentlicher Abruf historischer Kerzen (`GET /0/public/OHLC`, ohne Zugangsdaten).

use omnitrade_core::Candle;
use serde_json::Value;

use crate::binance::klines::get_text;
use crate::error::ExchangeError;
use crate::kraken::client::interval_minutes;

const BASE_URL: &str = "https://api.kraken.com";

/// Lädt die letzten abgeschlossenen Kerzen (älteste zuerst, höchstens `limit`).
///
/// Kraken liefert bis zu 720 Kerzen; die letzte ist noch offen und wird verworfen,
/// passend zur Stream-Semantik (nur abgeschlossene Kerzen).
///
/// # Errors
/// Fehler beim Abruf, bei nicht unterstütztem Intervall oder unlesbarer Antwort.
pub async fn fetch_klines(
    symbol: &str,
    interval: &str,
    limit: usize,
) -> Result<Vec<Candle>, ExchangeError> {
    let minutes = interval_minutes(interval).ok_or_else(|| {
        ExchangeError::InvalidRequest(format!(
            "Intervall '{interval}' wird von Kraken nicht unterstützt"
        ))
    })?;
    let url = format!("{BASE_URL}/0/public/OHLC");
    let body = get_text(
        &url,
        &[
            ("pair", symbol.to_uppercase().as_str()),
            ("interval", minutes.to_string().as_str()),
        ],
    )
    .await?;
    let mut candles = parse_ohlc(&body)?;
    candles.pop(); // offene Kerze
    let skip = candles.len().saturating_sub(limit);
    Ok(candles.split_off(skip))
}

/// Parst `{"error":[],"result":{"<PAIR>":[[time,o,h,l,c,vwap,vol,count],..],"last":N}}`.
fn parse_ohlc(body: &str) -> Result<Vec<Candle>, ExchangeError> {
    let parse_err = |reason: &str| ExchangeError::MessageParseFailed {
        raw: body.chars().take(200).collect(),
        reason: reason.to_string(),
    };
    let value: Value = serde_json::from_str(body).map_err(|e| parse_err(&e.to_string()))?;
    if let Some(errors) = value.get("error").and_then(Value::as_array) {
        if !errors.is_empty() {
            let msg: Vec<&str> = errors.iter().filter_map(Value::as_str).collect();
            return Err(ExchangeError::ApiError(msg.join("; ")));
        }
    }
    let rows = value
        .get("result")
        .and_then(Value::as_object)
        .and_then(|r| r.iter().find(|(k, _)| k.as_str() != "last"))
        .and_then(|(_, v)| v.as_array())
        .ok_or_else(|| parse_err("keine Kerzenliste im Ergebnis"))?;
    rows.iter()
        .map(|row| {
            let f = |i: usize| {
                row.get(i)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or_else(|| parse_err("ungültiges Kerzenfeld"))
            };
            let secs = row
                .get(0)
                .and_then(Value::as_u64)
                .ok_or_else(|| parse_err("ungültige Zeit"))?;
            Ok(Candle::new(secs * 1000, f(1)?, f(2)?, f(3)?, f(4)?, f(6)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"error":[],"result":{"XXBTZUSD":[
        [1688671200,"30306.1","30306.2","30305.7","30305.7","30306.1","3.39243896",23],
        [1688671260,"30305.7","30310.0","30300.0","30301.0","30305.0","1.5",10]
    ],"last":1688671260}}"#;

    #[test]
    fn parses_rows_to_candles_in_milliseconds() {
        // Act
        let candles = parse_ohlc(SAMPLE).expect("valid");

        // Assert
        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0].timestamp_ms, 1_688_671_200_000);
        assert_eq!(
            (
                candles[0].open,
                candles[0].high,
                candles[0].low,
                candles[0].close
            ),
            (30306.1, 30306.2, 30305.7, 30305.7)
        );
        assert_eq!(candles[0].volume, 3.39243896);
    }

    #[test]
    fn surfaces_api_errors_and_garbage() {
        match parse_ohlc(r#"{"error":["EQuery:Unknown asset pair"]}"#) {
            Err(ExchangeError::ApiError(m)) => assert!(m.contains("Unknown asset pair")),
            other => panic!("expected ApiError, got {other:?}"),
        }
        assert!(parse_ohlc("kein json").is_err());
        assert!(parse_ohlc(r#"{"error":[],"result":{"last":1}}"#).is_err());
        assert!(
            parse_ohlc(r#"{"error":[],"result":{"P":[["x","1","1","1","1","1","1",1]]}}"#).is_err()
        );
    }
}
