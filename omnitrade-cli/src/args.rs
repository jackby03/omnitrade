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
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_parse_backtest_all_flags() {
        // Arrange
        let args = vec![
            "omnitrade-cli",
            "backtest",
            "--script",
            "/path/to/script.omni",
            "--data",
            "/path/to/data.csv",
            "--initial-balance",
            "5000.0",
            "--fee-rate",
            "0.002",
            "--slippage-bps",
            "10.0",
            "--quantity",
            "0.05",
        ];

        // Act
        let parsed = CliArgs::try_parse_from(args)
            .expect("Should successfully parse backtest command with all flags");

        // Assert
        if let Commands::Backtest {
            script,
            data,
            initial_balance,
            fee_rate,
            slippage_bps,
            quantity,
        } = parsed.command
        {
            assert_eq!(script, PathBuf::from("/path/to/script.omni"));
            assert_eq!(data, PathBuf::from("/path/to/data.csv"));
            assert!((initial_balance - 5000.0).abs() < f64::EPSILON);
            assert!((fee_rate - 0.002).abs() < f64::EPSILON);
            assert!((slippage_bps - 10.0).abs() < f64::EPSILON);
            assert!((quantity - 0.05).abs() < f64::EPSILON);
        } else {
            unreachable!("Expected Commands::Backtest variant");
        }
    }

    #[test]
    fn test_parse_backtest_default_values() {
        // Arrange
        let args = vec![
            "omnitrade-cli",
            "backtest",
            "--script",
            "script.omni",
            "--data",
            "data.csv",
        ];

        // Act
        let parsed = CliArgs::try_parse_from(args)
            .expect("Should successfully parse backtest command with default flags");

        // Assert
        if let Commands::Backtest {
            script,
            data,
            initial_balance,
            fee_rate,
            slippage_bps,
            quantity,
        } = parsed.command
        {
            assert_eq!(script, PathBuf::from("script.omni"));
            assert_eq!(data, PathBuf::from("data.csv"));
            assert!((initial_balance - 10000.0).abs() < f64::EPSILON);
            assert!((fee_rate - 0.001).abs() < f64::EPSILON);
            assert!((slippage_bps - 5.0).abs() < f64::EPSILON);
            assert!((quantity - 1.0).abs() < f64::EPSILON);
        } else {
            unreachable!("Expected Commands::Backtest variant");
        }
    }

    #[test]
    fn test_parse_validate_command() {
        // Arrange
        let args = vec!["omnitrade-cli", "validate", "--script", "strategy.omni"];

        // Act
        let parsed =
            CliArgs::try_parse_from(args).expect("Should successfully parse validate command");

        // Assert
        if let Commands::Validate { script } = parsed.command {
            assert_eq!(script, PathBuf::from("strategy.omni"));
        } else {
            unreachable!("Expected Commands::Validate variant");
        }
    }

    #[test]
    fn test_parse_kill_reset_flag() {
        let parsed = CliArgs::try_parse_from(["omnitrade-cli", "kill", "--reset"])
            .expect("Should parse kill --reset");
        assert!(matches!(parsed.command, Commands::Kill { reset: true }));
    }

    #[test]
    fn test_parse_kill_command() {
        // Arrange
        let args = vec!["omnitrade-cli", "kill"];

        // Act
        let parsed = CliArgs::try_parse_from(args).expect("Should successfully parse kill command");

        // Assert
        assert!(
            matches!(parsed.command, Commands::Kill { reset: false }),
            "Expected Commands::Kill variant"
        );
    }

    #[test]
    fn test_missing_required_argument_returns_error() {
        // Arrange
        let args = vec!["omnitrade-cli", "backtest", "--script", "script.omni"];

        // Act
        let result = CliArgs::try_parse_from(args);

        // Assert
        assert!(
            result.is_err(),
            "Expected error due to missing --data argument"
        );
    }

    #[test]
    fn test_parse_live_command_with_script() {
        // Arrange
        let args = vec![
            "omnitrade-cli",
            "live",
            "--script",
            "strategy.omni",
            "--symbol",
            "BTCUSDT",
        ];

        // Act
        let parsed = CliArgs::try_parse_from(args)
            .expect("Should successfully parse live command with script");

        // Assert
        if let Commands::Live {
            script,
            grid_config,
            symbol,
            exchange,
            network,
            initial_capital,
            quantity,
            max_notional_usd,
            confirm_real_money,
            status_file,
            interval,
            ..
        } = parsed.command
        {
            assert_eq!(script, Some(PathBuf::from("strategy.omni")));
            assert_eq!(grid_config, None);
            assert_eq!(symbol, "BTCUSDT");
            assert_eq!(exchange, "binance");
            assert_eq!(network, "testnet");
            assert_eq!(initial_capital, None);
            assert_eq!(quantity, 0.001);
            assert_eq!(max_notional_usd, None);
            assert_eq!(status_file, None);
            assert_eq!(interval, "1m");
            assert!(!confirm_real_money);
        } else {
            unreachable!("Expected Commands::Live variant");
        }
    }

    #[test]
    fn test_parse_live_command_with_grid_config() {
        // Arrange
        let args = vec![
            "omnitrade-cli",
            "live",
            "--grid-config",
            "grid.json",
            "--symbol",
            "BTCUSDT",
        ];

        // Act
        let parsed = CliArgs::try_parse_from(args)
            .expect("Should successfully parse live command with grid config");

        // Assert
        if let Commands::Live {
            script,
            grid_config,
            symbol,
            exchange,
            network,
            initial_capital,
            quantity,
            max_notional_usd,
            confirm_real_money,
            status_file,
            interval,
            ..
        } = parsed.command
        {
            assert_eq!(script, None);
            assert_eq!(grid_config, Some(PathBuf::from("grid.json")));
            assert_eq!(symbol, "BTCUSDT");
            assert_eq!(exchange, "binance");
            assert_eq!(network, "testnet");
            assert_eq!(initial_capital, None);
            assert_eq!(quantity, 0.001);
            assert_eq!(max_notional_usd, None);
            assert_eq!(status_file, None);
            assert_eq!(interval, "1m");
            assert!(!confirm_real_money);
        } else {
            unreachable!("Expected Commands::Live variant");
        }
    }

    #[test]
    fn test_parse_live_command_all_flags() {
        // Arrange
        let args = vec![
            "omnitrade-cli",
            "live",
            "--script",
            "strategy.omni",
            "--symbol",
            "ETHUSDT",
            "--network",
            "testnet",
            "--initial-capital",
            "5000.0",
        ];

        // Act
        let parsed = CliArgs::try_parse_from(args)
            .expect("Should successfully parse live command with all flags");

        // Assert
        if let Commands::Live {
            script,
            grid_config,
            symbol,
            exchange,
            network,
            initial_capital,
            quantity,
            max_notional_usd,
            confirm_real_money,
            status_file,
            interval,
            ..
        } = parsed.command
        {
            assert_eq!(script, Some(PathBuf::from("strategy.omni")));
            assert_eq!(grid_config, None);
            assert_eq!(symbol, "ETHUSDT");
            assert_eq!(exchange, "binance");
            assert_eq!(network, "testnet");
            assert_eq!(initial_capital, Some(5000.0));
            assert_eq!(quantity, 0.001);
            assert_eq!(max_notional_usd, None);
            assert_eq!(status_file, None);
            assert_eq!(interval, "1m");
            assert!(!confirm_real_money);
        } else {
            unreachable!("Expected Commands::Live variant");
        }
    }

    #[test]
    fn test_parse_live_command_mainnet_flags() {
        // Arrange
        let args = vec![
            "omnitrade-cli",
            "live",
            "--script",
            "s.omni",
            "--symbol",
            "BTC-USDC",
            "--exchange",
            "okx",
            "--network",
            "mainnet",
            "--max-notional-usd",
            "20",
            "--confirm-real-money",
        ];

        // Act
        let parsed = CliArgs::try_parse_from(args).expect("Should parse mainnet flags");

        // Assert
        if let Commands::Live {
            max_notional_usd,
            confirm_real_money,
            network,
            ..
        } = parsed.command
        {
            assert_eq!(max_notional_usd, Some(20.0));
            assert!(confirm_real_money);
            assert_eq!(network, "mainnet");
        } else {
            unreachable!("Expected Commands::Live variant");
        }
    }

    #[test]
    fn test_parse_live_command_exchange_flag() {
        // Arrange
        let args = vec![
            "omnitrade-cli",
            "live",
            "--script",
            "s.omni",
            "--symbol",
            "BTC-USDT",
            "--exchange",
            "okx",
        ];

        // Act
        let parsed = CliArgs::try_parse_from(args).expect("Should parse --exchange");

        // Assert
        if let Commands::Live {
            exchange, symbol, ..
        } = parsed.command
        {
            assert_eq!(exchange, "okx");
            assert_eq!(symbol, "BTC-USDT");
        } else {
            unreachable!("Expected Commands::Live variant");
        }
    }

    #[test]
    fn parses_allow_shorts_flag() {
        // Arrange & Act
        let args = vec!["omnitrade-cli", "live", "--symbol", "BTCUSDT", "--allow-shorts"];
        let parsed = CliArgs::try_parse_from(args).expect("Should parse --allow-shorts");

        // Assert
        if let Commands::Live {
            allow_shorts, symbol, ..
        } = parsed.command
        {
            assert!(allow_shorts);
            assert_eq!(symbol, "BTCUSDT");
        } else {
            unreachable!("Expected Commands::Live variant");
        }

        // Ohne Flag: long-only bleibt der Standard.
        let parsed = CliArgs::try_parse_from(["omnitrade-cli", "live", "--symbol", "BTCUSDT"])
            .expect("Should parse without flag");
        if let Commands::Live { allow_shorts, .. } = parsed.command {
            assert!(!allow_shorts);
        } else {
            unreachable!("Expected Commands::Live variant");
        }
    }
}
