//! Binance Margin/Futures REST API client (Testnet-only).

use super::signing::BinanceSigner;
use crate::credentials::ApiCredentials;
use crate::error::ExchangeError;
use crate::trading::{Account, AccountBalance, ExchangeTrading, OrderRequest};
use omnitrade_core::{Order, OrderStatus, OrderType, Side, TimeInForce};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const FUTURES_TESTNET_URL: &str = "https://testnet.binancefuture.com";
const MARGIN_TESTNET_URL: &str = "https://testnet.binance.vision";

/// Binance Margin/Futures REST client (Testnet-only).
pub struct BinanceMarginClient {
    http_client: Client,
    base_url: String,
    credentials: ApiCredentials,
}

impl BinanceMarginClient {
    /// Create Futures trading client (Testnet-only). Hard gate: Mainnet rejected.
    pub fn new(credentials: ApiCredentials) -> Result<Self, ExchangeError> {
        if credentials.is_mainnet() {
            return Err(ExchangeError::InvalidRequest(
                "Margin/Futures trading is Testnet-only".to_string(),
            ));
        }
        credentials.validate().map_err(|e| {
            ExchangeError::InvalidCredentials(format!("Invalid credentials: {}", e))
        })?;
        Ok(Self {
            http_client: Client::new(),
            base_url: FUTURES_TESTNET_URL.to_string(),
            credentials,
        })
    }

    /// Create Margin trading client (SAPI endpoints, Testnet-only).
    pub fn new_margin(credentials: ApiCredentials) -> Result<Self, ExchangeError> {
        if credentials.is_mainnet() {
            return Err(ExchangeError::InvalidRequest(
                "Margin/Futures trading is Testnet-only".to_string(),
            ));
        }
        credentials.validate().map_err(|e| {
            ExchangeError::InvalidCredentials(format!("Invalid credentials: {}", e))
        })?;
        Ok(Self {
            http_client: Client::new(),
            base_url: MARGIN_TESTNET_URL.to_string(),
            credentials,
        })
    }

    async fn signed_request(
        &self,
        method: Method,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
    ) -> Result<reqwest::Response, ExchangeError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ExchangeError::InvalidRequest("System time error".to_string()))?
            .as_millis() as i64;

        let query = if let Some(p) = params {
            p.iter()
                .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                .collect::<Vec<_>>()
                .join("&")
        } else {
            String::new()
        };

        let signed_query =
            BinanceSigner::sign_request(&query, &self.credentials.secret, timestamp)?;
        // Die signierte Query gehört als ganzes an die URL: timestamp und signature
        // müssen Top-Level-Parameter sein, nicht in einem Feld verschachtelt.
        let url = format!("{}{}?{}", self.base_url, endpoint, signed_query);

        self.http_client
            .request(method, &url)
            .header("X-MBX-APIKEY", &self.credentials.key)
            .send()
            .await
            .map_err(|e| ExchangeError::ConnectionFailed {
                url,
                reason: e.to_string(),
            })
    }

    fn parse_order(&self, resp: &BinanceOrderResponse) -> Order {
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
                _ => OrderType::Market,
            },
            time_in_force: TimeInForce::GTC,
            price: resp.price,
            stop_price: resp.stop_price.unwrap_or(0.0),
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
            updated_at_ms: resp.update_time as u64,
            trailing_highest_price: None,
        }
    }
}

#[async_trait::async_trait]
impl ExchangeTrading for BinanceMarginClient {
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

        let mut params = HashMap::new();
        params.insert("symbol".to_string(), req.symbol.clone());
        params.insert(
            "side".to_string(),
            if req.side == Side::Buy { "BUY" } else { "SELL" }.to_string(),
        );
        params.insert(
            "type".to_string(),
            match req.order_type {
                OrderType::Limit => "LIMIT",
                _ => "MARKET",
            }
            .to_string(),
        );
        params.insert("quantity".to_string(), req.quantity.to_string());
        if let Some(price) = req.price {
            params.insert("price".to_string(), price.to_string());
        }

        let response = self
            .signed_request(Method::POST, "/fapi/v1/order", Some(&params))
            .await?;

        if !response.status().is_success() {
            return Err(ExchangeError::ApiError(
                response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string()),
            ));
        }

        response
            .json::<BinanceOrderResponse>()
            .await
            .map_err(|e| ExchangeError::MessageParseFailed {
                raw: e.to_string(),
                reason: "Failed to parse order response".to_string(),
            })
            .map(|body| self.parse_order(&body))
    }

    async fn cancel_order(&self, order_id: &str, symbol: &str) -> Result<(), ExchangeError> {
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        params.insert("orderId".to_string(), order_id.to_string());

        let response = self
            .signed_request(Method::DELETE, "/fapi/v1/order", Some(&params))
            .await?;

        if !response.status().is_success() {
            return Err(ExchangeError::ApiError(
                response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string()),
            ));
        }
        Ok(())
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>, ExchangeError> {
        let response = self
            .signed_request(Method::GET, "/fapi/v1/openOrders", None)
            .await?;

        if !response.status().is_success() {
            return Err(ExchangeError::ApiError(
                response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string()),
            ));
        }

        response
            .json::<Vec<BinanceOrderResponse>>()
            .await
            .map_err(|e| ExchangeError::MessageParseFailed {
                raw: e.to_string(),
                reason: "Failed to parse orders".to_string(),
            })
            .map(|orders| orders.iter().map(|o| self.parse_order(o)).collect())
    }

    async fn get_balance(&self) -> Result<Account, ExchangeError> {
        let response = self
            .signed_request(Method::GET, "/fapi/v1/account", None)
            .await?;

        if !response.status().is_success() {
            return Err(ExchangeError::ApiError(
                response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string()),
            ));
        }

        response
            .json::<BinanceMarginAccountResponse>()
            .await
            .map_err(|e| ExchangeError::MessageParseFailed {
                raw: e.to_string(),
                reason: "Failed to parse account".to_string(),
            })
            .map(|account_resp| {
                let mut balances = HashMap::new();
                for asset in account_resp.assets {
                    balances.insert(
                        asset.asset,
                        AccountBalance {
                            free: asset.wallet_balance,
                            locked: asset.unrealized_profit.abs(),
                        },
                    );
                }
                Account { balances }
            })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinanceOrderResponse {
    symbol: String,
    order_id: i64,
    client_order_id: String,
    #[serde(deserialize_with = "super::rest::de_f64")]
    price: f64,
    #[serde(deserialize_with = "super::rest::de_f64")]
    orig_qty: f64,
    #[serde(deserialize_with = "super::rest::de_f64")]
    executed_qty: f64,
    status: String,
    time_in_force: String,
    #[serde(rename = "type")]
    order_type: String,
    side: String,
    time: i64,
    update_time: i64,
    #[serde(default, deserialize_with = "de_opt_f64")]
    stop_price: Option<f64>,
}

/// Binance liefert Preise und Mengen als JSON-Strings, manchmal als Zahl; leer oder fehlend heißt `None`.
fn de_opt_f64<'de, D>(d: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    match Option::<serde_json::Value>::deserialize(d)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => Ok(n.as_f64()),
        Some(serde_json::Value::String(s)) if s.is_empty() => Ok(None),
        Some(serde_json::Value::String(s)) => s.parse().map(Some).map_err(D::Error::custom),
        Some(other) => Err(D::Error::custom(format!("unerwarteter Wert: {other}"))),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinanceMarginAccountResponse {
    assets: Vec<BinanceAsset>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BinanceAsset {
    asset: String,
    #[serde(deserialize_with = "super::rest::de_f64")]
    wallet_balance: f64,
    #[serde(deserialize_with = "super::rest::de_f64")]
    unrealized_profit: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_response_accepts_string_numbers() {
        let json = r#"{"symbol":"BTCUSDT","orderId":1,"clientOrderId":"c","price":"0.00000",
            "origQty":"0.001","executedQty":"0.001","status":"FILLED","timeInForce":"GTC",
            "type":"MARKET","side":"BUY","time":1,"updateTime":2,"stopPrice":""}"#;
        let r: BinanceOrderResponse = serde_json::from_str(json).expect("Binance-Strings");
        assert_eq!(r.orig_qty, 0.001);
        assert_eq!(r.executed_qty, 0.001);
        assert_eq!(r.stop_price, None);
    }
}
