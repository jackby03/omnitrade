//! Kraken REST client: read-only account access.
//!
//! Kraken Spot has no testnet. Until the project has an explicit mainnet approval
//! path for this exchange, order placement and cancellation are **refused**, and
//! `get_open_orders` is unsupported because Kraken order ids (`OXXXXX-XXXXX-XXXXXX`)
//! do not fit `omnitrade_core::Order::id` (`u64`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use omnitrade_core::Order;
use serde::Deserialize;

use crate::credentials::{ApiCredentials, Network};
use crate::error::ExchangeError;
use crate::kraken::signing::KrakenSigner;
use crate::trading::{Account, AccountBalance, ExchangeTrading, OrderRequest};

const BASE_URL: &str = "https://api.kraken.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Kraken REST client (read-only).
pub struct KrakenRestClient {
    http: reqwest::Client,
    base_url: String,
    credentials: ApiCredentials,
    last_nonce: AtomicU64,
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    error: Vec<String>,
    result: Option<T>,
}

impl KrakenRestClient {
    /// Creates a client. Kraken Spot only exists on mainnet, so testnet credentials are rejected.
    ///
    /// # Errors
    /// `InvalidRequest` for testnet credentials, `InvalidCredentials` for empty values,
    /// `ConnectionFailed` if the HTTP client cannot be built.
    pub fn new(credentials: ApiCredentials) -> Result<Self, ExchangeError> {
        if credentials.network == Network::Testnet {
            return Err(ExchangeError::InvalidRequest(
                "Kraken Spot hat kein Testnet; Zugänge müssen für mainnet hinterlegt sein"
                    .to_string(),
            ));
        }
        credentials
            .validate()
            .map_err(ExchangeError::InvalidCredentials)?;
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| ExchangeError::ConnectionFailed {
                url: BASE_URL.to_string(),
                reason: e.to_string(),
            })?;
        Ok(Self {
            http,
            base_url: BASE_URL.to_string(),
            credentials,
            last_nonce: AtomicU64::new(0),
        })
    }

    /// Strictly increasing nonce in milliseconds (Kraken rejects repeats and decreases).
    fn next_nonce(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut prev = self.last_nonce.load(Ordering::Relaxed);
        loop {
            let next = now.max(prev + 1);
            match self.last_nonce.compare_exchange_weak(
                prev,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return next,
                Err(actual) => prev = actual,
            }
        }
    }

    async fn private<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, ExchangeError> {
        let nonce = self.next_nonce();
        let body = format!("nonce={nonce}");
        let sign = KrakenSigner::sign(path, nonce, &body, &self.credentials.secret)?;
        let url = format!("{}{path}", self.base_url);
        let conn_err = |e: reqwest::Error| ExchangeError::ConnectionFailed {
            url: url.clone(),
            reason: e.to_string(),
        };
        let response = self
            .http
            .post(&url)
            .header("API-Key", &self.credentials.key)
            .header("API-Sign", sign)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(conn_err)?;
        let status = response.status();
        let text = response.text().await.map_err(conn_err)?;
        parse_envelope(status.is_success(), &text)
    }
}

fn parse_envelope<T: for<'de> Deserialize<'de>>(
    http_ok: bool,
    text: &str,
) -> Result<T, ExchangeError> {
    let envelope: Envelope<T> =
        serde_json::from_str(text).map_err(|e| ExchangeError::MessageParseFailed {
            raw: text.chars().take(200).collect(),
            reason: e.to_string(),
        })?;
    if !envelope.error.is_empty() {
        return Err(ExchangeError::ApiError(envelope.error.join("; ")));
    }
    match envelope.result {
        Some(result) if http_ok => Ok(result),
        _ => Err(ExchangeError::ApiError("Antwort ohne Ergebnis".to_string())),
    }
}

/// Converts Kraken's `{"ZUSD": "25435.2100", ...}` into an [`Account`] (nothing is locked
/// here: `Balance` reports totals, so `locked` stays 0).
fn balances_from(raw: HashMap<String, String>) -> Result<Account, ExchangeError> {
    let mut balances = HashMap::new();
    for (asset, value) in raw {
        let total: f64 = value
            .parse()
            .map_err(|_| ExchangeError::MessageParseFailed {
                raw: format!("{asset}={value}"),
                reason: "ungültiger Kontostand".to_string(),
            })?;
        balances.insert(
            asset,
            AccountBalance {
                free: total,
                locked: 0.0,
            },
        );
    }
    Ok(Account { balances })
}

fn refused(action: &str) -> ExchangeError {
    ExchangeError::InvalidRequest(format!(
        "Kraken: {action} ist gesperrt (Spot hat kein Testnet; Mainnet-Orders sind für diese Börse nicht freigegeben)"
    ))
}

#[async_trait::async_trait]
impl ExchangeTrading for KrakenRestClient {
    async fn place_order(&self, _req: &OrderRequest) -> Result<Order, ExchangeError> {
        Err(refused("Order-Platzierung"))
    }

    async fn cancel_order(&self, _order_id: &str, _symbol: &str) -> Result<(), ExchangeError> {
        Err(refused("Order-Stornierung"))
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>, ExchangeError> {
        Err(ExchangeError::InvalidRequest(
            "Kraken: offene Orders nicht unterstützt (Order-IDs sind Strings, Order::id ist u64)"
                .to_string(),
        ))
    }

    async fn get_balance(&self) -> Result<Account, ExchangeError> {
        let raw: HashMap<String, String> = self.private("/0/private/Balance").await?;
        balances_from(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnitrade_core::Side;

    const SECRET: &str =
        "kQH5HW/8p1uGOVjbgWA7FunAmGO8lsSUXNsu3eow76sz84Q18fWxnyRzBHCd3pd5nE9qa99HAZtuZuj6F1huXg==";

    fn client() -> KrakenRestClient {
        KrakenRestClient::new(ApiCredentials::new(
            "key".into(),
            SECRET.into(),
            Network::Mainnet,
        ))
        .expect("client")
    }

    #[test]
    fn rejects_testnet_and_empty_credentials() {
        let testnet = ApiCredentials::new("k".into(), SECRET.into(), Network::Testnet);
        assert!(matches!(
            KrakenRestClient::new(testnet),
            Err(ExchangeError::InvalidRequest(_))
        ));
        let empty = ApiCredentials::new(String::new(), SECRET.into(), Network::Mainnet);
        assert!(matches!(
            KrakenRestClient::new(empty),
            Err(ExchangeError::InvalidCredentials(_))
        ));
    }

    #[test]
    fn nonce_is_strictly_increasing() {
        let c = client();
        let a = c.next_nonce();
        let b = c.next_nonce();
        let d = c.next_nonce();
        assert!(a < b && b < d);
    }

    #[test]
    fn parses_balance_envelope() {
        // Act
        let raw: HashMap<String, String> = parse_envelope(
            true,
            r#"{"error":[],"result":{"ZUSD":"25435.2100","XXBT":"0.5"}}"#,
        )
        .expect("ok");
        let account = balances_from(raw).expect("balances");

        // Assert
        assert_eq!(account.balances["ZUSD"].total(), 25435.21);
        assert_eq!(account.balances["XXBT"].free, 0.5);
    }

    #[test]
    fn api_errors_are_surfaced() {
        let res: Result<HashMap<String, String>, _> =
            parse_envelope(true, r#"{"error":["EAPI:Invalid key"]}"#);
        match res {
            Err(ExchangeError::ApiError(msg)) => assert!(msg.contains("Invalid key")),
            other => panic!("expected ApiError, got {other:?}"),
        }
        assert!(parse_envelope::<HashMap<String, String>>(true, "kein json").is_err());
        assert!(balances_from(HashMap::from([("X".into(), "abc".into())])).is_err());
    }

    #[tokio::test]
    async fn orders_are_refused() {
        // Arrange
        let c = client();

        // Act & Assert
        let req = OrderRequest::market("XBTUSD".into(), Side::Buy, 1.0);
        assert!(c.place_order(&req).await.is_err());
        assert!(c.cancel_order("OABC", "XBTUSD").await.is_err());
        assert!(c.get_open_orders().await.is_err());
    }
}
