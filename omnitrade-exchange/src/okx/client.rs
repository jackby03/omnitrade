//! OKX WebSocket v5 streaming client (public market data).
//!
//! Candles live on the `/business` endpoint, order books on `/public`, so the client
//! opens one connection per endpoint that has subscriptions.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use omnitrade_core::Candle;
use serde_json::json;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::credentials::Network;
use crate::error::ExchangeError;
use crate::okx::router::OkxRouter;
use crate::traits::{DepthUpdate, ExchangeInfo, ExchangeStream};

const RAW_CHANNEL_CAPACITY: usize = 100;
/// OKX closes connections that stay silent for 30 s; a text `ping` keeps them open.
const PING_INTERVAL: Duration = Duration::from_secs(20);

/// Production WebSocket endpoints.
pub const PUBLIC_URL: &str = "wss://ws.okx.com:8443/ws/v5/public";
/// Production WebSocket endpoint for candles.
pub const BUSINESS_URL: &str = "wss://ws.okx.com:8443/ws/v5/business";
/// Demo-trading WebSocket endpoints.
pub const DEMO_PUBLIC_URL: &str = "wss://wspap.okx.com:8443/ws/v5/public";
/// Demo-trading WebSocket endpoint for candles.
pub const DEMO_BUSINESS_URL: &str = "wss://wspap.okx.com:8443/ws/v5/business";
/// EEA-Konten: Produktion (Hosts `wseea`, Keys aus dem EWR-Konto).
pub const EEA_PUBLIC_URL: &str = "wss://wseea.okx.com:8443/ws/v5/public";
/// EEA-Konten: Produktion, Kerzen-Kanal.
pub const EEA_BUSINESS_URL: &str = "wss://wseea.okx.com:8443/ws/v5/business";
/// EEA-Konten: Demo-Trading, Depth.
pub const EEA_DEMO_PUBLIC_URL: &str = "wss://wseeapap.okx.com:8443/ws/v5/public";
/// EEA-Konten: Demo-Trading, Kerzen-Kanal.
pub const EEA_DEMO_BUSINESS_URL: &str = "wss://wseeapap.okx.com:8443/ws/v5/business";

/// Maps an interval string to OKX's channel suffix (`1m`, `1H`, `1D`, ...).
pub(crate) fn interval_code(interval: &str) -> Option<&'static str> {
    match interval {
        "1m" => Some("1m"),
        "3m" => Some("3m"),
        "5m" => Some("5m"),
        "15m" => Some("15m"),
        "30m" => Some("30m"),
        "1h" => Some("1H"),
        "2h" => Some("2H"),
        "4h" => Some("4H"),
        "1d" => Some("1D"),
        "1w" => Some("1W"),
        _ => None,
    }
}

/// OKX-Instrumente sind `BASE-QUOTE` (z. B. `BTC-USDT`), Perpetuals
/// `BASE-QUOTE-SWAP`, datierte Futures `BASE-QUOTE-YYMMDD` (z. B.
/// `BTC-USD-261030`) und die EEA-XPERPs `BASE-QUOTE_UM_XPERP-YYMMDD`
/// (z. B. `SOL-USD_UM_XPERP-310404`).
fn validate_symbol(symbol: &str) -> Result<String, ExchangeError> {
    let upper = symbol.to_uppercase();
    let core = upper.strip_suffix("-SWAP").unwrap_or(upper.as_str());
    let digits6 = |s: &str| s.len() == 6 && s.chars().all(|c| c.is_ascii_digit());
    let ok = match core.split('-').collect::<Vec<_>>().as_slice() {
        [base, quote] if !base.is_empty() && !quote.is_empty() => true,
        // BASE-QUOTE_UM_XPERP-YYMMDD (EEA-Perpetual)
        [base, mid, expiry] if !base.is_empty() && mid.contains("_UM_XPERP") && digits6(expiry) => {
            true
        }
        // BASE-QUOTE-YYMMDD (datiertes Future)
        [base, quote, expiry]
            if !base.is_empty()
                && !quote.is_empty()
                && quote.chars().all(|c| c.is_ascii_alphanumeric())
                && digits6(expiry) =>
        {
            true
        }
        _ => false,
    };
    if ok {
        Ok(upper)
    } else {
        Err(ExchangeError::SubscriptionFailed {
            symbol: symbol.to_string(),
            reason: "OKX-Symbole haben die Form BASE-QUOTE, BASE-QUOTE-SWAP, \
                     BASE-QUOTE-YYMMDD oder BASE-QUOTE_UM_XPERP-YYMMDD"
                .to_string(),
        })
    }
}

/// Streaming client for OKX market data.
pub struct OkxClient {
    public_url: String,
    business_url: String,
    public_subs: Vec<String>,
    business_subs: Vec<String>,
    network: Network,
    router: Option<OkxRouter>,
    raw_tx: mpsc::Sender<String>,
    tasks: Vec<JoinHandle<()>>,
    router_task: Option<JoinHandle<Result<(), ExchangeError>>>,
    shutdown_tx: Option<watch::Sender<bool>>,
    is_connected: bool,
}

impl OkxClient {
    /// Creates a client for production endpoints.
    pub fn new() -> Self {
        Self::with_urls(PUBLIC_URL, BUSINESS_URL)
    }

    /// Creates a client whose endpoints match the network (`Testnet` = demo trading).
    pub fn for_network(network: Network) -> Self {
        Self::for_region(network, false)
    }

    /// Like [`Self::for_network`], but `eea = true` selects the `wseea`/`wseeapap` hosts
    /// used by accounts registered in the EEA.
    pub fn for_region(network: Network, eea: bool) -> Self {
        let mut client = match (network, eea) {
            (Network::Testnet, false) => Self::with_urls(DEMO_PUBLIC_URL, DEMO_BUSINESS_URL),
            (Network::Testnet, true) => Self::with_urls(EEA_DEMO_PUBLIC_URL, EEA_DEMO_BUSINESS_URL),
            (Network::Mainnet, false) => Self::new(),
            (Network::Mainnet, true) => Self::with_urls(EEA_PUBLIC_URL, EEA_BUSINESS_URL),
        };
        client.network = network;
        client
    }

    /// Creates a client for custom endpoints (tests).
    pub fn with_urls(public_url: impl Into<String>, business_url: impl Into<String>) -> Self {
        let (raw_tx, raw_rx) = mpsc::channel(RAW_CHANNEL_CAPACITY);
        Self {
            public_url: public_url.into(),
            business_url: business_url.into(),
            public_subs: Vec::new(),
            business_subs: Vec::new(),
            network: Network::Mainnet,
            router: Some(OkxRouter::new(raw_rx)),
            raw_tx,
            tasks: Vec::new(),
            router_task: None,
            shutdown_tx: None,
            is_connected: false,
        }
    }

    /// Subscribe messages sent on the `/public` endpoint after connecting.
    pub fn public_subscriptions(&self) -> &[String] {
        &self.public_subs
    }

    /// Subscribe messages sent on the `/business` endpoint after connecting.
    pub fn business_subscriptions(&self) -> &[String] {
        &self.business_subs
    }

    async fn open(
        &mut self,
        url: String,
        subs: Vec<String>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), ExchangeError> {
        let (write, read) = verbinde(&url, &subs).await?;
        let raw_tx = self.raw_tx.clone();
        self.tasks.push(tokio::spawn(lesen_mit_neuaufbau(
            url, subs, raw_tx, shutdown, write, read,
        )));
        Ok(())
    }
}

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
type WsRead = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

/// Wartezeit vor dem naechsten Verbindungsversuch: 1, 2, 4, 8, 16, hoechstens 30 s.
pub(crate) fn wiederverbindung_wartezeit(versuch: u32) -> Duration {
    let sekunden = 1u64 << versuch.saturating_sub(1).min(5);
    Duration::from_secs(sekunden.min(30))
}

/// Baut eine Verbindung auf und schickt die Abo-Nachrichten.
async fn verbinde(url: &str, subs: &[String]) -> Result<(WsSink, WsRead), ExchangeError> {
    let (ws, _) = tokio_tungstenite::connect_async(url).await.map_err(|e| {
        ExchangeError::ConnectionFailed {
            url: url.to_string(),
            reason: e.to_string(),
        }
    })?;
    let (mut write, read) = ws.split();
    for sub in subs {
        write
            .send(Message::Text(sub.clone().into()))
            .await
            .map_err(|e| ExchangeError::ConnectionFailed {
                url: url.to_string(),
                reason: format!("subscribe failed: {e}"),
            })?;
    }
    info!("Connected to OKX WebSocket at '{url}'");
    Ok((write, read))
}

/// Liest bis zum Fehler und baut die Verbindung dann neu auf.
///
/// Ein einzelner Lesefehler (z. B. `Connection reset without closing handshake`)
/// beendete bisher nur diese Task. Der Kanal zum Router blieb dabei offen, das
/// `candle_rx.recv()` im Live-Loop wartete also ewig: der Prozess lebte, schrieb
/// keine Statusdatei mehr und handelte nicht mehr (21.10.2026, 5,5 h still).
/// Deshalb endet diese Task nur noch, wenn der Aufrufer abschaltet oder der
/// Router verschwindet - sonst wird bis zum Erfolg neu verbunden.
async fn lesen_mit_neuaufbau(
    url: String,
    subs: Vec<String>,
    raw_tx: mpsc::Sender<String>,
    mut shutdown: watch::Receiver<bool>,
    mut write: WsSink,
    mut read: WsRead,
) {
    let mut versuch: u32 = 0;
    loop {
        let mut ping = tokio::time::interval(PING_INTERVAL);
        let grund = loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    let _ = write.close().await;
                    return;
                }
                _ = ping.tick() => {
                    if write.send(Message::Text("ping".into())).await.is_err() {
                        break String::from("ping fehlgeschlagen");
                    }
                }
                msg = read.next() => match msg {
                    Some(Ok(Message::Text(text))) => {
                        if text.as_str() == "pong" {
                            continue;
                        }
                        if raw_tx.send(text.to_string()).await.is_err() {
                            return;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = write.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        break String::from("Verbindung geschlossen");
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        break format!("Lesefehler: {err}");
                    }
                }
            }
        };

        versuch += 1;
        let warte = wiederverbindung_wartezeit(versuch);
        warn!(
            "OKX WebSocket {url}: {grund} - neuer Aufbau in {} s (Versuch {versuch})",
            warte.as_secs()
        );
        tokio::select! {
            _ = tokio::time::sleep(warte) => {}
            _ = shutdown.changed() => return,
        }
        loop {
            match verbinde(&url, &subs).await {
                Ok((neuer_sender, neuer_leser)) => {
                    write = neuer_sender;
                    read = neuer_leser;
                    versuch = 0;
                    break;
                }
                Err(err) => {
                    versuch += 1;
                    let warte = wiederverbindung_wartezeit(versuch);
                    warn!(
                        "OKX WebSocket {url}: {err} - naechster Versuch in {} s",
                        warte.as_secs()
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(warte) => {}
                        _ = shutdown.changed() => return,
                    }
                }
            }
        }
    }
}

impl Default for OkxClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExchangeStream for OkxClient {
    async fn connect(&mut self) -> Result<(), ExchangeError> {
        if self.is_connected || self.router.is_none() {
            return Err(ExchangeError::ConnectionFailed {
                url: self.public_url.clone(),
                reason: "client is already connected".to_string(),
            });
        }
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        let endpoints = [
            (self.public_url.clone(), self.public_subs.clone()),
            (self.business_url.clone(), self.business_subs.clone()),
        ];
        for (url, subs) in endpoints {
            if subs.is_empty() {
                continue;
            }
            if let Err(err) = self.open(url, subs, shutdown_rx.clone()).await {
                self.disconnect().await?;
                return Err(err);
            }
        }

        if let Some(mut router) = self.router.take() {
            self.router_task = Some(tokio::spawn(async move { router.run().await }));
        }
        self.is_connected = true;
        Ok(())
    }

    /// Must be called *before* [`connect()`](Self::connect). Only confirmed candles arrive.
    async fn subscribe_candles(
        &mut self,
        symbol: &str,
        interval: &str,
    ) -> Result<mpsc::Receiver<Candle>, ExchangeError> {
        let symbol = validate_symbol(symbol)?;
        let code = interval_code(interval).ok_or_else(|| ExchangeError::SubscriptionFailed {
            symbol: symbol.clone(),
            reason: format!("Intervall '{interval}' wird von OKX nicht unterstützt"),
        })?;
        let router = self
            .router
            .as_mut()
            .ok_or_else(|| ExchangeError::SubscriptionFailed {
                symbol: symbol.clone(),
                reason: "cannot subscribe after connection has been established".to_string(),
            })?;
        let rx = router.register_candle_channel(&symbol);
        self.business_subs.push(
            json!({"op": "subscribe", "args": [{"channel": format!("candle{code}"), "instId": symbol}]})
                .to_string(),
        );
        Ok(rx)
    }

    /// Must be called *before* [`connect()`](Self::connect). The first message per
    /// instrument is a snapshot, later ones are deltas (size 0 removes a level).
    async fn subscribe_depth(
        &mut self,
        symbol: &str,
    ) -> Result<mpsc::Receiver<DepthUpdate>, ExchangeError> {
        let symbol = validate_symbol(symbol)?;
        let router = self
            .router
            .as_mut()
            .ok_or_else(|| ExchangeError::SubscriptionFailed {
                symbol: symbol.clone(),
                reason: "cannot subscribe after connection has been established".to_string(),
            })?;
        let rx = router.register_depth_channel(&symbol);
        self.public_subs.push(
            json!({"op": "subscribe", "args": [{"channel": "books", "instId": symbol}]})
                .to_string(),
        );
        Ok(rx)
    }

    /// Rueckfallweg, wenn der WebSocket still bleibt: bestaetigte Kerzen per REST.
    async fn recent_candles(
        &self,
        symbol: &str,
        interval: &str,
        limit: usize,
    ) -> Result<Vec<Candle>, ExchangeError> {
        crate::okx::klines::fetch_klines(self.network, symbol, interval, limit).await
    }

    async fn disconnect(&mut self) -> Result<(), ExchangeError> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        for task in self.tasks.drain(..) {
            let _ = tokio::time::timeout(Duration::from_millis(500), task).await;
        }
        self.is_connected = false;
        if let Some(task) = self.router_task.take() {
            let _ = tokio::time::timeout(Duration::from_millis(500), task).await;
        }
        Ok(())
    }
}

impl ExchangeInfo for OkxClient {
    fn name(&self) -> &str {
        "OKX"
    }

    fn supported_intervals(&self) -> &[&str] {
        &["1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "1d", "1w"]
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
