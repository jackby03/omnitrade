//! OKX REST client. SPOT uses `tdMode=cash`; SWAP (perpetual) uses `tdMode=isolated`
//! and the SWAP-specific helpers in `swap_client`.
//!
//! `Network::Testnet` means OKX **demo trading**: requests carry
//! `x-simulated-trading: 1` and need demo API keys. OKX additionally requires an API
//! passphrase, which `ApiCredentials` has no field for, so it is passed separately.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

use omnitrade_core::{Order, OrderStatus, OrderType, Side, TimeInForce};
use reqwest::Method;
use serde_json::{json, Value};

use crate::credentials::{ApiCredentials, Network};
use crate::error::ExchangeError;
use crate::okx::signing::{iso_timestamp, OkxSigner};
use crate::okx::swap_client::{is_contract_id, InstrumentCache};
use crate::trading::{Account, AccountBalance, ExchangeTrading, OrderRequest, PositionSnapshot};

const BASE_URL: &str = "https://www.okx.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// OKX error code for "order does not exist".
const ORDER_NOT_FOUND: &str = "51603";

/// OKX REST client.
pub struct OkxRestClient {
    http: reqwest::Client,
    base_url: String,
    credentials: ApiCredentials,
    passphrase: String,
    pub(crate) instruments: InstrumentCache,
}

impl OkxRestClient {
    /// Creates a client.
    ///
    /// # Errors
    /// `InvalidCredentials` for empty key, secret or passphrase, `ConnectionFailed` if the
    /// HTTP client cannot be built.
    pub fn new(credentials: ApiCredentials, passphrase: String) -> Result<Self, ExchangeError> {
        credentials
            .validate()
            .map_err(ExchangeError::InvalidCredentials)?;
        if passphrase.is_empty() {
            return Err(ExchangeError::InvalidCredentials(
                "API passphrase cannot be empty".to_string(),
            ));
        }
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
            passphrase,
            instruments: RwLock::new(HashMap::new()),
        })
    }

    /// Uses another official OKX API domain, e.g. `https://eea.okx.com` (EEA accounts) or
    /// `https://us.okx.com`. Keys exist only on the domain of their account region.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    fn is_demo(&self) -> bool {
        self.credentials.network == Network::Testnet
    }

    /// Sends a signed request and returns the validated `data` array of the envelope.
    pub(crate) async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Vec<Value>, ExchangeError> {
        let body_text = body.map(Value::to_string).unwrap_or_default();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let timestamp = iso_timestamp(now_ms);
        let sign = OkxSigner::sign(
            &timestamp,
            method.as_str(),
            path,
            &body_text,
            &self.credentials.secret,
        )?;
        let url = format!("{}{path}", self.base_url);
        let conn_err = |e: reqwest::Error| ExchangeError::ConnectionFailed {
            url: url.clone(),
            reason: e.to_string(),
        };
        let mut request = self
            .http
            .request(method, &url)
            .header("OK-ACCESS-KEY", &self.credentials.key)
            .header("OK-ACCESS-SIGN", sign)
            .header("OK-ACCESS-TIMESTAMP", timestamp)
            .header("OK-ACCESS-PASSPHRASE", &self.passphrase)
            .header("Content-Type", "application/json");
        if self.is_demo() {
            request = request.header("x-simulated-trading", "1");
        }
        if body.is_some() {
            request = request.body(body_text);
        }
        let response = request.send().await.map_err(conn_err)?;
        let text = response.text().await.map_err(conn_err)?;
        parse_envelope(&text)
    }
}

/// Validates the `{"code","msg","data"}` envelope. A non-zero `code` or a per-item
/// `sCode != "0"` becomes an error; the code is kept in the message.
fn parse_envelope(text: &str) -> Result<Vec<Value>, ExchangeError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| ExchangeError::MessageParseFailed {
            raw: text.chars().take(200).collect(),
            reason: e.to_string(),
        })?;
    let code = value.get("code").and_then(Value::as_str).unwrap_or("");
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if code != "0" {
        let detail = data
            .first()
            .and_then(|d| d.get("sMsg"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        let msg = value.get("msg").and_then(Value::as_str).unwrap_or("");
        return Err(ExchangeError::ApiError(
            format!("{code}: {msg} {detail}").trim().to_string(),
        ));
    }
    if let Some(item) = data.first() {
        let s_code = item.get("sCode").and_then(Value::as_str).unwrap_or("0");
        if s_code != "0" {
            let s_msg = item.get("sMsg").and_then(Value::as_str).unwrap_or("");
            return Err(ExchangeError::ApiError(format!("{s_code}: {s_msg}")));
        }
    }
    Ok(data)
}

fn str_f64(v: &Value, key: &str) -> f64 {
    v.get(key)
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
}

fn str_u64(v: &Value, key: &str) -> u64 {
    v.get(key)
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Maps an OKX order object (from `orders-pending` or `order`) to an [`Order`].
fn parse_order(v: &Value) -> Option<Order> {
    let id = v.get("ordId")?.as_str()?.parse::<u64>().ok()?;
    let created = str_u64(v, "cTime");
    let updated = str_u64(v, "uTime");
    Some(Order {
        id,
        symbol: v.get("instId")?.as_str()?.to_string(),
        side: if v.get("side")?.as_str()? == "buy" {
            Side::Buy
        } else {
            Side::Sell
        },
        order_type: match v.get("ordType").and_then(Value::as_str) {
            Some("market") => OrderType::Market,
            _ => OrderType::Limit,
        },
        time_in_force: TimeInForce::GTC,
        price: str_f64(v, "px"),
        stop_price: 0.0,
        quantity: str_f64(v, "sz"),
        filled_quantity: str_f64(v, "accFillSz"),
        status: match v.get("state").and_then(Value::as_str) {
            Some("partially_filled") => OrderStatus::PartiallyFilled,
            Some("filled") => OrderStatus::Filled,
            Some("canceled") | Some("mmp_canceled") => OrderStatus::Cancelled,
            _ => OrderStatus::New,
        },
        created_at_ms: created,
        updated_at_ms: if updated > 0 { updated } else { created },
        trailing_highest_price: None,
    })
}

fn parse_balance(data: &[Value]) -> Account {
    let mut balances = std::collections::HashMap::new();
    for detail in data
        .iter()
        .filter_map(|d| d.get("details").and_then(Value::as_array))
        .flatten()
    {
        if let Some(ccy) = detail.get("ccy").and_then(Value::as_str) {
            balances.insert(
                ccy.to_string(),
                AccountBalance {
                    free: str_f64(detail, "availBal"),
                    locked: str_f64(detail, "frozenBal"),
                },
            );
        }
    }
    Account { balances }
}

#[async_trait::async_trait]
impl ExchangeTrading for OkxRestClient {
    async fn place_order(&self, req: &OrderRequest) -> Result<Order, ExchangeError> {
        if req.quantity <= 0.0 {
            return Err(ExchangeError::InvalidRequest(
                "Quantity must be positive".to_string(),
            ));
        }
        let (ord_type, px) = match (&req.order_type, req.price) {
            (OrderType::Market, _) => ("market", None),
            (OrderType::Limit, Some(p)) if p > 0.0 => ("limit", Some(p)),
            (OrderType::Limit, _) => {
                return Err(ExchangeError::InvalidRequest(
                    "Limit orders require price".to_string(),
                ))
            }
            (other, _) => {
                return Err(ExchangeError::InvalidRequest(format!(
                    "OKX: Ordertyp {other:?} wird nicht unterstützt"
                )))
            }
        };
        let body = {
            // Prefer cached instrument type; fall back to symbol heuristic.
            let is_contract = self
                .instruments
                .read()
                .ok()
                .and_then(|c| c.get(&req.symbol).cloned())
                .map(|i| i.inst_type.is_contract_based())
                .unwrap_or_else(|| is_contract_id(&req.symbol));
            if is_contract {
                self.swap_order_body(req, px)?
            } else {
                let mut body = json!({
                    "instId": req.symbol,
                    "tdMode": "cash",
                    "side": if req.side == Side::Buy { "buy" } else { "sell" },
                    "ordType": ord_type,
                    "sz": req.quantity.to_string(),
                });
                if let Some(price) = px {
                    body["px"] = json!(price.to_string());
                } else {
                    // Without this, market buy's size is read as quote currency.
                    body["tgtCcy"] = json!("base_ccy");
                }
                body
            }
        };
        let data = self
            .call(Method::POST, "/api/v5/trade/order", Some(&body))
            .await?;
        let ord_id = data
            .first()
            .and_then(|d| d.get("ordId"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| ExchangeError::ApiError("Antwort ohne verwertbare ordId".to_string()))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Ok(Order {
            id: ord_id,
            symbol: req.symbol.clone(),
            side: req.side,
            order_type: req.order_type.clone(),
            time_in_force: TimeInForce::GTC,
            price: px.unwrap_or(0.0),
            stop_price: 0.0,
            quantity: req.quantity,
            filled_quantity: 0.0,
            status: OrderStatus::New,
            created_at_ms: now,
            updated_at_ms: now,
            trailing_highest_price: None,
        })
    }

    async fn cancel_order(&self, order_id: &str, symbol: &str) -> Result<(), ExchangeError> {
        let body = json!({"instId": symbol, "ordId": order_id});
        self.call(Method::POST, "/api/v5/trade/cancel-order", Some(&body))
            .await
            .map(|_| ())
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>, ExchangeError> {
        // Ohne instType-Filter: OKX liefert SPOT und SWAP gemeinsam, SWAP wird unten
        // in Basiswährung zurückgerechnet.
        let data = self
            .call(Method::GET, "/api/v5/trade/orders-pending", None)
            .await?;
        // Eine fremde Order (anderes Instrument, ohne geladene Regeln) darf den Abruf
        // nicht zu Fall bringen: sonst bleibt auch der Not-Aus bei eigenen Orders stecken.
        Ok(data
            .iter()
            .filter_map(parse_order)
            .filter_map(|o| match self.to_base_units(o) {
                Ok(order) => Some(order),
                Err(e) => {
                    tracing::warn!("Offene Order übersprungen (Umrechnung fehlgeschlagen): {e}");
                    None
                }
            })
            .collect())
    }

    async fn get_balance(&self) -> Result<Account, ExchangeError> {
        let data = self
            .call(Method::GET, "/api/v5/account/balance", None)
            .await?;
        Ok(parse_balance(&data))
    }

    async fn get_order(
        &self,
        order_id: &str,
        symbol: &str,
    ) -> Result<Option<Order>, ExchangeError> {
        let path = format!(
            "/api/v5/trade/order?instId={}&ordId={}",
            urlencoding::encode(symbol),
            urlencoding::encode(order_id)
        );
        match self.call(Method::GET, &path, None).await {
            Ok(data) => data
                .first()
                .and_then(parse_order)
                .map(|o| self.to_base_units(o))
                .transpose(),
            Err(ExchangeError::ApiError(msg)) if msg.starts_with(ORDER_NOT_FOUND) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn position_snapshot(&self, symbol: &str) -> Result<PositionSnapshot, ExchangeError> {
        if is_contract_id(symbol) {
            self.fetch_swap_position(symbol).await
        } else {
            Ok(PositionSnapshot::default())
        }
    }
}

#[cfg(test)]
#[path = "rest_tests.rs"]
mod tests;
