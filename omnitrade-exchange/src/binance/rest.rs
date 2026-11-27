//! Binance REST API client for Spot trading.
//! Supports Testnet and Mainnet with automatic URL selection.

use super::signing::BinanceSigner;
use crate::credentials::ApiCredentials;
use crate::error::ExchangeError;
use crate::trading::{Account, AccountBalance, ExchangeTrading, OrderRequest};
use omnitrade_core::{Order, OrderStatus, OrderType, Side, TimeInForce};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Toleranz der Börse für den Zeitstempel (ms). Fängt kleine Uhrabweichungen ab.
const RECV_WINDOW_MS: u64 = 5000;

/// Binance liefert Zahlen als JSON-Strings ("0.00100000"); akzeptiert beides.
pub(crate) fn de_f64<'de, D>(d: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(f64),
        Str(String),
    }
    match NumOrStr::deserialize(d)? {
        NumOrStr::Num(n) => Ok(n),
        NumOrStr::Str(s) => s.parse::<f64>().map_err(serde::de::Error::custom),
    }
}

/// Lokale Uhr in Millisekunden seit der Unix-Epoche.
fn local_millis() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

/// Binance REST API base URLs.
const TESTNET_REST_URL: &str = "https://testnet.binance.vision";
const MAINNET_REST_URL: &str = "https://api.binance.com";

/// Binance REST API client.
pub struct BinanceRestClient {
    http_client: Client,
    base_url: String,
    credentials: ApiCredentials,
    /// Abweichung Serverzeit minus lokale Uhr (ms), einmalig abgeglichen.
    time_offset_ms: AtomicI64,
    time_synced: AtomicBool,
}

/// Antwort von `GET /api/v3/time`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerTime {
    server_time: i64,
}

impl BinanceRestClient {
    /// Create a new Binance REST client.
    pub fn new(credentials: ApiCredentials) -> Result<Self, ExchangeError> {
        credentials.validate().map_err(|e| {
            ExchangeError::InvalidCredentials(format!("Invalid credentials: {}", e))
        })?;

        let base_url = if credentials.is_testnet() {
            TESTNET_REST_URL.to_string()
        } else {
            MAINNET_REST_URL.to_string()
        };

        let http_client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ExchangeError::InvalidRequest(format!("HTTP client: {}", e)))?;

        Ok(Self {
            http_client,
            base_url,
            credentials,
            time_offset_ms: AtomicI64::new(0),
            time_synced: AtomicBool::new(false),
        })
    }

    /// Gleicht die lokale Uhr einmalig mit der Serverzeit ab. Schlägt der Abruf
    /// fehl, bleibt der Offset 0 und `recvWindow` fängt kleine Abweichungen ab.
    async fn sync_time(&self) {
        let url = format!("{}/api/v3/time", self.base_url);
        let local = local_millis().unwrap_or(0);
        let server = match self.http_client.get(&url).send().await {
            Ok(resp) => resp.json::<ServerTime>().await.ok(),
            Err(_) => None,
        };
        if let Some(server) = server {
            let now = local_millis().unwrap_or(local);
            // Mittelwert aus Anfangs- und Endzeit gleicht die Laufzeit des Aufrufs aus.
            self.time_offset_ms
                .store(server.server_time - (local + now) / 2, Ordering::Relaxed);
        }
        self.time_synced.store(true, Ordering::Relaxed);
    }

    /// Send a signed request to the Binance API.
    async fn signed_request(
        &self,
        method: Method,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
    ) -> Result<reqwest::Response, ExchangeError> {
        if !self.time_synced.load(Ordering::Relaxed) {
            self.sync_time().await;
        }
        let timestamp = local_millis()
            .ok_or_else(|| ExchangeError::InvalidRequest("System time error".to_string()))?
            + self.time_offset_ms.load(Ordering::Relaxed);

        let mut pairs: Vec<String> = params
            .map(|p| {
                p.iter()
                    .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                    .collect()
            })
            .unwrap_or_default();
        pairs.push(format!("recvWindow={RECV_WINDOW_MS}"));
        let query = pairs.join("&");

        let signed_query =
            BinanceSigner::sign_request(&query, &self.credentials.secret, timestamp)?;
        let url = format!("{}/api/v3{}", self.base_url, endpoint);
        let full_url = format!("{}?{}", url, signed_query);

        self.http_client
            .request(method, &full_url)
            .header("X-MBX-APIKEY", &self.credentials.key)
            .send()
            .await
            .map_err(|e| ExchangeError::ConnectionFailed {
                url,
                reason: e.to_string(),
            })
    }

    /// Parse Binance order response into our Order domain type.
    fn parse_order_response(&self, resp: &BinanceOrderResponse) -> Order {
        Order {
            id: resp.order_id as u64,
            symbol: resp.symbol.clone(),
            side: if resp.side == "BUY" {
                Side::Buy
            } else {
                Side::Sell
            },
            order_type: match resp.order_type.as_str() {
                "LIMIT" => OrderType::Limit,
                "MARKET" => OrderType::Market,
                _ => OrderType::Market,
            },
            time_in_force: TimeInForce::GTC,
            price: resp.price,
            stop_price: 0.0,
            status: match resp.status.as_str() {
                "NEW" => OrderStatus::New,
                "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
                "FILLED" => OrderStatus::Filled,
                "CANCELED" => OrderStatus::Cancelled,
                "REJECTED" => OrderStatus::Rejected,
                _ => OrderStatus::New,
            },
            quantity: resp.orig_qty,
            filled_quantity: resp.executed_qty,
            created_at_ms: resp.time as u64,
            updated_at_ms: if resp.update_time > 0 {
                resp.update_time as u64
            } else {
                resp.time as u64
            },
            trailing_highest_price: None,
        }
    }
}

#[async_trait::async_trait]
impl ExchangeTrading for BinanceRestClient {
    async fn place_order(&self, req: &OrderRequest) -> Result<Order, ExchangeError> {
        if req.quantity <= 0.0 {
            return Err(ExchangeError::InvalidRequest(
                "Quantity must be positive".to_string(),
            ));
        }

        if req.order_type == OrderType::Limit && req.price.is_none() {
            return Err(ExchangeError::InvalidRequest(
                "Limit orders require price".to_string(),
            ));
        }

        // Stop-, Take-Profit- und Trailing-Orders bekommen hier keinen Auslöser. Als MARKET
        // geschickt würde eine Schutz-Order sofort ausgeführt, also wie beim OKX-Adapter ablehnen.
        if !matches!(req.order_type, OrderType::Market | OrderType::Limit) {
            return Err(ExchangeError::InvalidRequest(format!(
                "Binance-Adapter unterstützt den Ordertyp {:?} nicht",
                req.order_type
            )));
        }

        let mut params = HashMap::new();
        params.insert("symbol".to_string(), req.symbol.clone());
        params.insert(
            "side".to_string(),
            if req.side == Side::Buy { "BUY" } else { "SELL" }.to_string(),
        );
        params.insert(
            "type".to_string(),
            match req.order_type {
                OrderType::Market => "MARKET",
                OrderType::Limit => "LIMIT",
                _ => "MARKET",
            }
            .to_string(),
        );
        params.insert("quantity".to_string(), req.quantity.to_string());

        if let Some(price) = req.price {
            params.insert("price".to_string(), price.to_string());
        }
        if req.order_type == OrderType::Limit {
            params.insert("timeInForce".to_string(), "GTC".to_string());
        }

        let response = self
            .signed_request(Method::POST, "/order", Some(&params))
            .await?;

        if !response.status().is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ExchangeError::ApiError(body));
        }

        let body = response.json::<BinanceOrderResponse>().await.map_err(|e| {
            ExchangeError::MessageParseFailed {
                raw: e.to_string(),
                reason: "Failed to parse order response".to_string(),
            }
        })?;

        Ok(self.parse_order_response(&body))
    }

    async fn cancel_order(&self, order_id: &str, symbol: &str) -> Result<(), ExchangeError> {
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        params.insert("orderId".to_string(), order_id.to_string());

        let response = self
            .signed_request(Method::DELETE, "/order", Some(&params))
            .await?;

        if !response.status().is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ExchangeError::ApiError(body));
        }

        Ok(())
    }

    async fn get_order(
        &self,
        order_id: &str,
        symbol: &str,
    ) -> Result<Option<Order>, ExchangeError> {
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        params.insert("orderId".to_string(), order_id.to_string());

        let response = self
            .signed_request(Method::GET, "/order", Some(&params))
            .await?;

        if !response.status().is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            // -2013: Order existiert nicht.
            if body.contains("-2013") {
                return Ok(None);
            }
            return Err(ExchangeError::ApiError(body));
        }

        let order = response.json::<BinanceOrderResponse>().await.map_err(|e| {
            ExchangeError::MessageParseFailed {
                raw: e.to_string(),
                reason: "Failed to parse order response".to_string(),
            }
        })?;

        Ok(Some(self.parse_order_response(&order)))
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>, ExchangeError> {
        let response = self
            .signed_request(Method::GET, "/openOrders", None)
            .await?;

        if !response.status().is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ExchangeError::ApiError(body));
        }

        let orders: Vec<BinanceOrderResponse> =
            response
                .json()
                .await
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: e.to_string(),
                    reason: "Failed to parse orders response".to_string(),
                })?;

        Ok(orders
            .iter()
            .map(|o| self.parse_order_response(o))
            .collect())
    }

    async fn get_balance(&self) -> Result<Account, ExchangeError> {
        let response = self.signed_request(Method::GET, "/account", None).await?;

        if !response.status().is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ExchangeError::ApiError(body));
        }

        let account_resp: BinanceAccountResponse =
            response
                .json()
                .await
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: e.to_string(),
                    reason: "Failed to parse account response".to_string(),
                })?;

        let mut balances = HashMap::new();
        for balance in account_resp.balances {
            balances.insert(
                balance.asset.clone(),
                AccountBalance {
                    free: balance.free,
                    locked: balance.locked,
                },
            );
        }

        Ok(Account { balances })
    }
}

/// Binance API order response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinanceOrderResponse {
    symbol: String,
    order_id: i64,
    #[serde(default)]
    client_order_id: String,
    #[serde(deserialize_with = "de_f64")]
    price: f64,
    #[serde(deserialize_with = "de_f64")]
    orig_qty: f64,
    #[serde(deserialize_with = "de_f64")]
    executed_qty: f64,
    status: String,
    #[serde(default)]
    time_in_force: String,
    #[serde(rename = "type")]
    order_type: String,
    side: String,
    /// Fehlt in der Antwort auf POST /order (dort `transactTime`).
    #[serde(default, alias = "transactTime")]
    time: i64,
    #[serde(default)]
    update_time: i64,
}

/// Binance API account response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinanceAccountResponse {
    balances: Vec<BinanceBalance>,
    #[serde(default)]
    account_commission_btc: String,
    #[serde(default)]
    account_commission_bnb: String,
    #[serde(default)]
    account_commission_usdt: String,
}

/// Binance balance entry.
#[derive(Debug, Serialize, Deserialize)]
struct BinanceBalance {
    asset: String,
    #[serde(deserialize_with = "de_f64")]
    free: f64,
    #[serde(deserialize_with = "de_f64")]
    locked: f64,
}

#[cfg(test)]
mod tests {
    #[test]
    fn server_time_response_parses() {
        let t: ServerTime = serde_json::from_str(r#"{"serverTime":1499827319559}"#)
            .expect("serverTime should parse");
        assert_eq!(t.server_time, 1_499_827_319_559);
    }

    use super::*;

    #[test]
    fn test_base_url_testnet() {
        let creds = ApiCredentials::new(
            "test_key".to_string(),
            "test_secret".to_string(),
            crate::Network::Testnet,
        );
        let client = BinanceRestClient::new(creds).expect("Failed to create client");
        assert_eq!(client.base_url, TESTNET_REST_URL);
    }

    #[test]
    fn test_base_url_mainnet() {
        let creds = ApiCredentials::new(
            "test_key".to_string(),
            "test_secret".to_string(),
            crate::Network::Mainnet,
        );
        let client = BinanceRestClient::new(creds).expect("Failed to create client");
        assert_eq!(client.base_url, MAINNET_REST_URL);
    }

    #[test]
    fn test_parse_order_response_with_string_numbers() {
        let json = r#"{"symbol":"BTCUSDT","orderId":28,"transactTime":1507725176595,
            "price":"0.00000000","origQty":"10.00000000","executedQty":"10.00000000",
            "status":"FILLED","timeInForce":"GTC","type":"MARKET","side":"SELL"}"#;
        let resp: BinanceOrderResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(resp.orig_qty, 10.0);
        assert_eq!(resp.time, 1507725176595);
    }

    #[test]
    fn test_parse_balances_with_string_numbers() {
        let json = r#"{"balances":[{"asset":"BTC","free":"0.5","locked":"0.1"}]}"#;
        let resp: BinanceAccountResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(resp.balances[0].free, 0.5);
    }

    #[test]
    fn test_client_creation_empty_key() {
        let creds = ApiCredentials::new(
            "".to_string(),
            "secret".to_string(),
            crate::Network::Testnet,
        );
        let result = BinanceRestClient::new(creds);
        assert!(result.is_err());
    }
}
