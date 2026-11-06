//! Command-line argument definitions and parsing for `omnitrade-cli`.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Command line arguments for `omnitrade-cli`.
#[derive(Debug, Parser, PartialEq)]
#[command(
    name = "omnitrade-cli",
    version,
    about = "Headless CLI runner for omnitrade"
)]
pub struct CliArgs {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// Available subcommands for `omnitrade-cli`.
#[derive(Debug, Subcommand, PartialEq)]
pub enum Commands {
    /// Run a backtest using a strategy script and market data.
    Backtest {
        /// Path to the strategy script.
        #[arg(long, value_name = "PATH")]
        script: PathBuf,

        /// Path to the market data file.
        #[arg(long, value_name = "PATH")]
        data: PathBuf,

        /// Initial account balance.
        #[arg(long, value_name = "AMOUNT", default_value_t = 10000.0)]
        initial_balance: f64,

        /// Trading fee rate.
        #[arg(long, value_name = "RATE", default_value_t = 0.001)]
        fee_rate: f64,

        /// Slippage in basis points.
        #[arg(long, value_name = "BPS", default_value_t = 5.0)]
        slippage_bps: f64,

        /// Order quantity per signal (default 1.0).
        #[arg(long, value_name = "AMOUNT", default_value_t = 1.0)]
        quantity: f64,
    },
    /// Validate a strategy script for syntax and structural correctness.
    Validate {
        /// Path to the strategy script.
        #[arg(long, value_name = "PATH")]
        script: PathBuf,
    },
    /// Execute live trading on Binance with a strategy script or grid config.
    ///
    /// Connects to a live exchange, subscribes to market data, and places orders
    /// based on strategy signals OR grid configuration (mutually exclusive).
    /// Enforces risk limits via RiskGuard.
    /// Testnet by default; mainnet requires explicit flag.
    Live {
        /// Path to the strategy script (.omni file). Mutually exclusive with --grid-config.
        #[arg(long, value_name = "PATH")]
        script: Option<PathBuf>,

        /// Path to grid config file (JSON). Mutually exclusive with --script.
        #[arg(long, value_name = "PATH")]
        grid_config: Option<PathBuf>,

        /// Trading symbol in the exchange's notation (BTCUSDT, BTC-USDT, BTC/USD).
        #[arg(long, value_name = "SYMBOL")]
        symbol: String,

        /// Exchange: "binance", "okx" or "kraken" (Kraken has no testnet, trading is refused).
        #[arg(long, value_name = "EXCHANGE", default_value = "binance")]
        exchange: String,

        /// Network: "testnet" or "mainnet".
        #[arg(long, value_name = "NETWORK", default_value = "testnet")]
        network: String,

        /// Capital per position in quote currency (default 500). Long-only:
        /// a buy spends it when flat, a sell closes the position.
        #[arg(long, value_name = "AMOUNT")]
        initial_capital: Option<f64>,

        /// Menge je Market-Order in der Basiswährung (Default 0.001).
        #[arg(long, value_name = "QTY", default_value_t = 0.001)]
        quantity: f64,

        /// Deckel für die vom Lauf aufgebaute Position in der Quote-Währung. Auf Mainnet
        /// Pflicht (höchstens 100); auf dem Testnet optional.
        #[arg(long, value_name = "AMOUNT")]
        max_notional_usd: Option<f64>,

        /// Bestätigt, dass auf Mainnet echtes Geld eingesetzt wird (nur OKX).
        #[arg(long)]
        confirm_real_money: bool,

        /// Erlaubt Kurzpositionen: Ein Sell-Signal bei (nahezu) flacher Position
        /// öffnet einen Short (OKX net mode). Standard ist long-only (Haussatz
        /// C.2: Shorts erst nach belegtem Long-Lauf). Der Positionsdeckel
        /// `--max-notional-usd` gilt dann in beide Richtungen.
        #[arg(long)]
        allow_shorts: bool,
        /// Write a JSON status snapshot to this file while running
        /// (read-only view for dashboards; written atomically).
        #[arg(long, value_name = "PATH")]
        status_file: Option<PathBuf>,

        /// Candle length the strategy runs on, e.g. "1m", "15m", "1h", "4h",
        /// "1d". On OKX the last 300 closed candles preload the indicators.
        #[arg(long, value_name = "INTERVAL", default_value = "1m")]
        interval: String,
    },
    /// Emergency kill switch to halt all trading operations.
    ///
    /// Stops a running live trading process (no new orders, open orders are
    /// cancelled) and blocks new live runs until `--reset`. Holdings are not sold.
    /// This is a safety mechanism and should be used when risk limits are breached
    /// or market conditions require immediate halt.
    Kill {
        /// Hebt einen gesetzten Not-Aus wieder auf.
        #[arg(long)]
        reset: bool,
    },
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
