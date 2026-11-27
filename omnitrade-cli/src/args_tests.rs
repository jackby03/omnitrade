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
    let parsed = CliArgs::try_parse_from(args).expect("Should successfully parse validate command");

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
    let parsed =
        CliArgs::try_parse_from(args).expect("Should successfully parse live command with script");

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
    let args = vec![
        "omnitrade-cli",
        "live",
        "--symbol",
        "BTCUSDT",
        "--allow-shorts",
    ];
    let parsed = CliArgs::try_parse_from(args).expect("Should parse --allow-shorts");

    // Assert
    if let Commands::Live {
        allow_shorts,
        symbol,
        ..
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
