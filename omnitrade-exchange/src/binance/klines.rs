//! Öffentliche Marktdaten per REST ohne Zugangsdaten: Kerzen (`/api/v3/klines`) und
//! Order-Book-Snapshot (`/api/v3/depth`).

use crate::credentials::Network;
use crate::error::ExchangeError;
use omnitrade_core::Candle;
use serde_json::Value;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const TESTNET_REST_URL: &str = "https://testnet.binance.vision";
const MAINNET_REST_URL: &str = "https://api.binance.com";
/// Obergrenze der Börse pro Abruf.
const MAX_LIMIT: usize = 1000;

pub(crate) fn base_url(network: Network) -> &'static str {
    match network {
        Network::Testnet => TESTNET_REST_URL,
        Network::Mainnet => MAINNET_REST_URL,
    }
}

/// Order-Book-Snapshot: Gebote absteigend, Angebote aufsteigend, je `(Preis, Menge)`.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthSnapshot {
    /// Gebote `(Preis, Menge)`.
    pub bids: Vec<(f64, f64)>,
    /// Angebote `(Preis, Menge)`.
    pub asks: Vec<(f64, f64)>,
}

/// Lädt einen Order-Book-Snapshot mit bis zu `limit` Ebenen je Seite.
pub async fn fetch_depth_snapshot(
    network: Network,
    symbol: &str,
    limit: usize,
) -> Result<DepthSnapshot, ExchangeError> {
    let url = format!("{}/api/v3/depth", base_url(network));
    let limit = limit.clamp(1, 5000).to_string();
    let body = get_text(
        &url,
        &[
            ("symbol", symbol.to_uppercase().as_str()),
            ("limit", limit.as_str()),
        ],
    )
    .await?;
    parse_depth(&body)
}

/// Lädt die letzten `limit` Kerzen (älteste zuerst). Die letzte kann noch offen sein.
pub async fn fetch_klines(
    network: Network,
    symbol: &str,
    interval: &str,
    limit: usize,
) -> Result<Vec<Candle>, ExchangeError> {
    let url = format!("{}/api/v3/klines", base_url(network));
    let limit = limit.clamp(1, MAX_LIMIT).to_string();
    let body = get_text(
        &url,
        &[
            ("symbol", symbol.to_uppercase().as_str()),
            ("interval", interval),
            ("limit", limit.as_str()),
        ],
    )
    .await?;
    let mut candles = parse_klines(&body)?;
    // Die letzte Kerze ist bei einem Start mitten in der Periode noch offen. Sie gehört
    // nicht in die Historie: die Live-Schleife würde die fertige Kerze sonst verwerfen.
    let schliesst_spaeter = serde_json::from_str::<Vec<Vec<Value>>>(&body)
        .ok()
        .and_then(|zeilen| zeilen.last().and_then(|z| z.get(6)).and_then(Value::as_i64));
    if let Some(schluss_ms) = schliesst_spaeter {
        let jetzt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if schluss_ms > jetzt {
            candles.pop();
        }
    }
    Ok(candles)
}

pub(crate) async fn get_text(url: &str, query: &[(&str, &str)]) -> Result<String, ExchangeError> {
    get_text_with_headers(url, query, &[]).await
}

/// Wie [`get_text`], mit zusätzlichen Headern (z. B. `x-simulated-trading` für OKX-Demo).
pub(crate) async fn get_text_with_headers(
    url: &str,
    query: &[(&str, &str)],
    headers: &[(&str, &str)],
) -> Result<String, ExchangeError> {
    let conn_err = |e: reqwest::Error| ExchangeError::ConnectionFailed {
        url: url.to_string(),
        reason: e.to_string(),
    };
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(conn_err)?;
    let mut request = client.get(url).query(query);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await.map_err(conn_err)?;
    let status = response.status();
    let body = response.text().await.map_err(conn_err)?;
    if !status.is_success() {
        return Err(ExchangeError::ApiError(format!("HTTP {status}: {body}")));
    }
    Ok(body)
}

/// Parst die Depth-Antwort `{"lastUpdateId":..,"bids":[["p","q"],..],"asks":[..]}`.
fn parse_depth(body: &str) -> Result<DepthSnapshot, ExchangeError> {
    let parse_err = |reason: &str| ExchangeError::MessageParseFailed {
        raw: body.chars().take(200).collect(),
        reason: reason.to_string(),
    };
    let value: Value = serde_json::from_str(body).map_err(|e| parse_err(&e.to_string()))?;
    let side = |key: &str| -> Result<Vec<(f64, f64)>, ExchangeError> {
        value
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| parse_err("Seite fehlt"))?
            .iter()
            .map(|level| {
                let num = |i: usize| {
                    level
                        .get(i)
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<f64>().ok())
                        .ok_or_else(|| parse_err("ungültige Ebene"))
                };
                Ok((num(0)?, num(1)?))
            })
            .collect()
    };
    Ok(DepthSnapshot {
        bids: side("bids")?,
        asks: side("asks")?,
    })
}

/// Parst die Klines-Antwort: Array von Arrays `[openTime, open, high, low, close, volume, ...]`.
fn parse_klines(body: &str) -> Result<Vec<Candle>, ExchangeError> {
    let parse_err = |reason: &str| ExchangeError::MessageParseFailed {
        raw: body.chars().take(200).collect(),
        reason: reason.to_string(),
    };
    let rows: Vec<Vec<Value>> =
        serde_json::from_str(body).map_err(|e| parse_err(&e.to_string()))?;
    rows.iter()
        .map(|row| {
            let num = |i: usize| -> Result<f64, ExchangeError> {
                row.get(i)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or_else(|| parse_err("ungültiges Kerzenfeld"))
            };
            let open_time = row
                .first()
                .and_then(Value::as_u64)
                .ok_or_else(|| parse_err("ungültige Öffnungszeit"))?;
            Ok(Candle::new(
                open_time,
                num(1)?,
                num(2)?,
                num(3)?,
                num(4)?,
                num(5)?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
        [1499040000000,"0.01634790","0.80000000","0.01575800","0.01577100","148976.11427815",1499644799999,"2434.19055334",308,"1756.87402397","28.46694368","0"],
        [1499644800000,"0.01577100","0.01600000","0.01500000","0.01550000","100.0",1500249599999,"1.0",5,"1","1","0"]
    ]"#;

    #[test]
    fn parses_rows_into_candles_in_order() {
        // Act
        let candles = parse_klines(SAMPLE).expect("valid klines");

        // Assert
        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0].timestamp_ms, 1499040000000);
        assert_eq!(candles[0].open, 0.0163479);
        assert_eq!(candles[0].high, 0.8);
        assert_eq!(candles[0].low, 0.015758);
        assert_eq!(candles[0].close, 0.015771);
        assert_eq!(candles[0].volume, 148976.11427815);
        assert_eq!(candles[1].close, 0.0155);
    }

    #[test]
    fn rejects_malformed_rows() {
        assert!(parse_klines("[[1,\"x\",\"1\",\"1\",\"1\",\"1\"]]").is_err());
        assert!(parse_klines("[[\"t\",\"1\",\"1\",\"1\",\"1\",\"1\"]]").is_err());
        assert!(parse_klines("{\"code\":-1121}").is_err());
    }

    #[test]
    fn empty_array_is_empty_vec() {
        assert!(parse_klines("[]").expect("valid").is_empty());
    }

    #[test]
    fn base_url_matches_network() {
        assert_eq!(base_url(Network::Testnet), TESTNET_REST_URL);
        assert_eq!(base_url(Network::Mainnet), MAINNET_REST_URL);
    }

    #[test]
    fn parses_depth_snapshot() {
        // Act
        let snap = parse_depth(
            r#"{"lastUpdateId":1,"bids":[["100.5","2"],["100.0","1"]],"asks":[["101.0","3"]]}"#,
        )
        .expect("valid depth");

        // Assert
        assert_eq!(snap.bids, vec![(100.5, 2.0), (100.0, 1.0)]);
        assert_eq!(snap.asks, vec![(101.0, 3.0)]);
    }

    #[test]
    fn rejects_malformed_depth() {
        assert!(parse_depth("{\"bids\":[[\"1\",\"x\"]],\"asks\":[]}").is_err());
        assert!(parse_depth("{\"code\":-1121}").is_err());
    }
}
