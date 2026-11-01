//! Gemeinsame Test-Doubles für die Live-Schleife: Stream, Trading und Harness.

use super::*;
use async_trait::async_trait;
use omnitrade_core::{Candle, OrderType};
use omnitrade_exchange::{Account, AccountBalance, DepthUpdate, ExchangeError};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub(crate) const BUY_BELOW_40K: &str = "if close() < 40000 { buy() }";
pub(crate) const SELL_ABOVE_40K: &str = "if close() > 40000 { sell() }";

pub(crate) type Log = Arc<Mutex<Vec<String>>>;
/// Stream-Mock: liefert vorbereitete Kerzen und protokolliert die Aufrufreihenfolge.
pub(crate) struct MockStream {
    pub(crate) candles: Vec<Candle>,
    pub(crate) log: Log,
}

#[async_trait]
impl ExchangeStream for MockStream {
    async fn connect(&mut self) -> Result<(), ExchangeError> {
        self.log.lock().expect("log").push("connect".into());
        Ok(())
    }

    async fn subscribe_candles(
        &mut self,
        _symbol: &str,
        _interval: &str,
    ) -> Result<mpsc::Receiver<Candle>, ExchangeError> {
        self.log.lock().expect("log").push("subscribe".into());
        let (tx, rx) = mpsc::channel(self.candles.len().max(1));
        for c in self.candles.drain(..) {
            tx.try_send(c).expect("channel capacity");
        }
        Ok(rx)
    }

    async fn subscribe_depth(
        &mut self,
        _symbol: &str,
    ) -> Result<mpsc::Receiver<DepthUpdate>, ExchangeError> {
        Err(ExchangeError::ChannelClosed)
    }

    async fn disconnect(&mut self) -> Result<(), ExchangeError> {
        self.log.lock().expect("log").push("disconnect".into());
        Ok(())
    }
}

/// Trading-Mock: konfigurierbarer Kontostand, protokolliert platzierte Orders.
pub(crate) struct MockTrading {
    pub(crate) usdt: Option<f64>,
    /// Gehaltene BTC (Basis-Bestand); 0 = kein Eintrag im Konto.
    pub(crate) btc: f64,
    /// Position, die die BOERSE meldet (vorzeichenbehaftet, Basiswaehrung).
    /// Unabhaengig davon, was dieser Lauf selbst geordert hat - genau das ist
    /// der Punkt beim Buchabgleich.
    pub(crate) pos_qty: f64,
    /// Waehrung, in der das Konto seinen Bestand fuehrt (Standard USDT).
    /// XPERP-Konten fuehren USDC, waehrend das Symbol `USD` sagt.
    pub(crate) waehrung: &'static str,
    /// uPnL, die die Boerse je Abruf meldet (Reihe, letzter Wert bleibt stehen).
    /// Damit laesst sich der Ablauf "erst Peak, dann Verlust" nachstellen.
    pub(crate) pos_upl: Arc<Mutex<(Vec<f64>, usize)>>,
    pub(crate) placed: Arc<Mutex<Vec<OrderRequest>>>,
    pub(crate) cancelled: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ExchangeTrading for MockTrading {
    async fn place_order(
        &self,
        req: &OrderRequest,
    ) -> Result<omnitrade_core::Order, ExchangeError> {
        self.placed.lock().expect("placed").push(req.clone());
        Ok(omnitrade_core::Order {
            id: 1,
            symbol: req.symbol.clone(),
            side: req.side,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::GTC,
            price: 0.0,
            stop_price: 0.0,
            quantity: req.quantity,
            filled_quantity: req.quantity,
            status: OrderStatus::Filled,
            created_at_ms: 0,
            updated_at_ms: 0,
            trailing_highest_price: None,
        })
    }

    async fn cancel_order(&self, order_id: &str, _symbol: &str) -> Result<(), ExchangeError> {
        self.cancelled
            .lock()
            .expect("cancelled")
            .push(order_id.to_string());
        Ok(())
    }

    async fn get_open_orders(&self) -> Result<Vec<omnitrade_core::Order>, ExchangeError> {
        Ok(vec![omnitrade_core::Order {
            id: 77,
            symbol: "BTCUSDT".to_string(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GTC,
            price: 1.0,
            stop_price: 0.0,
            quantity: 1.0,
            filled_quantity: 0.0,
            status: OrderStatus::New,
            created_at_ms: 0,
            updated_at_ms: 0,
            trailing_highest_price: None,
        }])
    }

    async fn position_snapshot(
        &self,
        _symbol: &str,
    ) -> Result<omnitrade_exchange::PositionSnapshot, ExchangeError> {
        let upl = {
            let mut z = self.pos_upl.lock().expect("upl");
            let i = z.1.min(z.0.len().saturating_sub(1));
            let wert = z.0.get(i).copied().unwrap_or(0.0);
            z.1 += 1;
            wert
        };
        // Bei offener Position verlangt der Guard Liq- und Markpreis (fail closed);
        // die Attrappe liefert einen weiten Abstand, damit der Abstandstest gruen ist.
        let (liq, mark) = if self.pos_qty == 0.0 {
            (None, None)
        } else {
            (Some(1.0), Some(1_000.0))
        };
        Ok(omnitrade_exchange::PositionSnapshot {
            is_perpetual: true,
            base_qty: self.pos_qty,
            unrealized_pnl: upl,
            liq_price: liq,
            mark_price: mark,
        })
    }

    async fn get_balance(&self) -> Result<Account, ExchangeError> {
        match self.usdt {
            Some(free) => {
                let mut balances = HashMap::from([(
                    self.waehrung.to_string(),
                    AccountBalance { free, locked: 0.0 },
                )]);
                if self.btc > 0.0 {
                    balances.insert(
                        "BTC".to_string(),
                        AccountBalance {
                            free: self.btc,
                            locked: 0.0,
                        },
                    );
                }
                Ok(Account { balances })
            }
            None => Err(ExchangeError::ChannelClosed),
        }
    }
}

pub(crate) struct Harness {
    pub(crate) script: PathBuf,
    pub(crate) log: Log,
    pub(crate) placed: Arc<Mutex<Vec<OrderRequest>>>,
    pub(crate) cancelled: Arc<Mutex<Vec<String>>>,
    pub(crate) kill: PathBuf,
    pub(crate) cap: Option<f64>,
    pub(crate) allow_shorts: bool,
    pub(crate) settle: Option<&'static str>,
    pub(crate) pos_qty: f64,
    pub(crate) status_pfad: Option<PathBuf>,
    pub(crate) symbol: &'static str,
    pub(crate) konto_waehrung: &'static str,
    pub(crate) upl_reihe: Vec<f64>,
}

impl Harness {
    pub(crate) fn with_cap(mut self, cap: f64) -> Self {
        self.cap = Some(cap);
        self
    }

    /// Erlaubt Kurzpositionen (Sell bei flacher Position öffnet Short).
    pub(crate) fn with_allow_shorts(mut self) -> Self {
        self.allow_shorts = true;
        self
    }

    pub(crate) fn new(name: &str, script: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "omnitrade-liveloop-{}-{name}.omni",
            std::process::id()
        ));
        fs::write(&path, script).expect("write script");
        Self {
            script: path,
            log: Arc::default(),
            placed: Arc::default(),
            cancelled: Arc::default(),
            kill: std::env::temp_dir().join(format!(
                "omnitrade-liveloop-{}-{name}.kill",
                std::process::id()
            )),
            cap: None,
            allow_shorts: false,
            settle: None,
            pos_qty: 0.0,
            status_pfad: None,
            symbol: "BTCUSDT",
            konto_waehrung: "USDT",
            upl_reihe: Vec::new(),
        }
    }

    /// Laesst die Boerse je Abruf diese uPnL melden (letzter Wert bleibt).
    pub(crate) fn with_upl_reihe(mut self, reihe: &[f64]) -> Self {
        self.upl_reihe = reihe.to_vec();
        self
    }

    /// Handeltssymbol des Laufs (Standard `BTCUSDT`); Perpetuals brauchen ihre
    /// volle Kennung, z. B. `SOL-USD_UM_XPERP-310404`.
    pub(crate) fn with_symbol(mut self, symbol: &'static str) -> Self {
        self.symbol = symbol;
        self
    }

    /// Waehrung, in der das Mock-Konto seinen Bestand fuehrt (Standard USDT).
    pub(crate) fn with_konto_waehrung(mut self, ccy: &'static str) -> Self {
        self.konto_waehrung = ccy;
        self
    }

    /// Schreibt den Status-Snapshot in eine Datei, damit ein Test das Buch lesen kann.
    pub(crate) fn with_status_file(mut self, name: &str) -> Self {
        self.status_pfad = Some(std::env::temp_dir().join(format!(
            "omnitrade-liveloop-{}-{name}-status.json",
            std::process::id()
        )));
        self
    }

    /// Setzt die Abrechnungswaehrung des Instruments (XPERP: USDC).
    pub(crate) fn with_settle(mut self, ccy: &'static str) -> Self {
        self.settle = Some(ccy);
        self
    }

    /// Laesst die Boerse eine offene Position melden.
    pub(crate) fn with_position(mut self, qty: f64) -> Self {
        self.pos_qty = qty;
        self
    }

    pub(crate) async fn run(
        &self,
        closes: &[f64],
        usdt: Option<f64>,
        network: Network,
        quantity: f64,
    ) -> Result<(), LiveLoopError> {
        let candles = closes
            .iter()
            .enumerate()
            .map(|(i, &p)| Candle::new(i as u64 * 60_000, p, p + 1.0, p - 1.0, p, 100.0))
            .collect();
        let stream = MockStream {
            candles,
            log: self.log.clone(),
        };
        let trading = MockTrading {
            usdt,
            btc: 0.0,
            pos_qty: self.pos_qty,
            waehrung: self.konto_waehrung,
            pos_upl: Arc::new(Mutex::new((self.upl_reihe.clone(), 0))),
            placed: self.placed.clone(),
            cancelled: self.cancelled.clone(),
        };
        let script = self.script.to_string_lossy().to_string();
        let mut status = crate::status::StatusWriter::new(
            self.status_pfad.clone(),
            "okx",
            self.symbol,
            "testnet",
            "test.omni",
            1000.0,
        );
        let config = LiveConfig {
            script_path: &script,
            symbol: self.symbol,
            network,
            initial_capital: 1000.0,
            order_quantity: quantity,
            kill_file: &self.kill,
            max_notional: self.cap,
            interval: "1m",
            warmup: &[],
            allow_shorts: self.allow_shorts,
        };
        let mut guard = RiskGuard::testnet();
        live_loop(
            &config,
            self.settle,
            stream,
            trading,
            &mut guard,
            &mut status,
        )
        .await
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.script);
        let _ = fs::remove_file(&self.kill);
        if let Some(pfad) = &self.status_pfad {
            let _ = fs::remove_file(pfad);
        }
    }
}
pub(crate) fn trading_with(usdt: f64, btc: f64) -> MockTrading {
    MockTrading {
        usdt: Some(usdt),
        btc,
        pos_qty: 0.0,
        waehrung: "USDT",
        pos_upl: Arc::default(),
        placed: Arc::default(),
        cancelled: Arc::default(),
    }
}
