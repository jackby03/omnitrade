use super::*;

fn sample_csv_data() -> &'static str {
    "timestamp_ms,open,high,low,close,volume\n\
     1000,100.0,105.0,95.0,102.0,500.0\n\
     2000,102.0,108.0,100.0,106.0,600.0\n\
     3000,106.0,110.0,104.0,109.0,700.0\n\
     4000,109.0,115.0,108.0,112.0,800.0\n\
     5000,112.0,120.0,110.0,118.0,900.0"
}

fn helper_rising_candles() -> Vec<Candle> {
    vec![
        Candle::new(1000, 100.0, 105.0, 95.0, 100.0, 500.0),
        Candle::new(2000, 110.0, 115.0, 105.0, 110.0, 600.0),
        Candle::new(3000, 120.0, 125.0, 115.0, 120.0, 700.0),
        Candle::new(4000, 130.0, 135.0, 125.0, 130.0, 800.0),
        Candle::new(5000, 140.0, 145.0, 135.0, 140.0, 900.0),
    ]
}

#[test]
fn test_csv_5_rows_parses_into_5_candle_structs() {
    // Arrange & Act
    let candles = load_candles_from_reader(sample_csv_data().as_bytes())
        .expect("CSV reading and parsing should succeed");

    // Assert
    assert_eq!(candles.len(), 5);
    assert_eq!(candles[0].timestamp_ms, 1000);
    assert!((candles[0].open - 100.0).abs() < f64::EPSILON);
    assert!((candles[4].close - 118.0).abs() < f64::EPSILON);
}

#[test]
fn test_malformed_csv_row_returns_error() {
    // Arrange
    let csv_data = "timestamp_ms,open,high,low,close,volume\n1000,100.0,INVALID,95.0,102.0,500.0";

    // Act & Assert
    assert!(load_candles_from_reader(csv_data.as_bytes()).is_err());
}

#[test]
fn test_indicator_warmup_skips_early_candles() {
    // A script with period indicators must not abort the backtest during warmup:
    // EvalError ("insufficient candles") is treated as "no signal yet".
    let candles: Vec<Candle> = (0..40)
        .map(|i| {
            let price = 100.0 + i as f64;
            Candle::new(i * 1000, price, price + 1.0, price - 1.0, price, 100.0)
        })
        .collect();
    let cfg = BacktestConfig {
        script_path: PathBuf::from("unused"),
        data_path: PathBuf::from("unused"),
        initial_balance: 200_000.0,
        fee_rate: 0.001,
        slippage_bps: 5.0,
        symbol: "BTCUSDT".to_string(),
        quantity: 1.0,
    };
    let script = "if ema(20) > ema(50) && rsi(14) < 30 { buy() }
        if ema(20) < ema(50) && rsi(14) > 70 { sell() }
";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg)
        .expect("backtest must complete despite indicator warmup");
    assert!(result.final_balance > 0.0);
}

#[test]
fn test_quantity_scales_pnl_linearly() {
    // 0.5 BTC auf 100 -> 111 Long: halber PnL des 1.0-Tests (5.3945).
    let candles = vec![
        Candle::new(1000, 100.0, 101.0, 99.0, 100.0, 500.0),
        Candle::new(2000, 111.0, 112.0, 110.0, 111.0, 600.0),
    ];
    let cfg = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        200_000.0,
        0.001,
        0.0,
        0.5,
    );
    let script = "if close() < 105 { buy() }
";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg)
        .expect("backtest must complete");
    let expected = 0.5 * ((111.0 - 100.0) - (111.0 + 100.0) * 0.001);
    assert!((result.total_pnl - expected).abs() < 1e-6);
    assert!((result.final_balance - (cfg.initial_balance + expected)).abs() < 1e-6);
}

#[test]
fn test_stop_loss_hits_and_closes_long_at_stop_price() {
    // Long bei 100, stop_loss(95): naechste Kerze low 94 -> Close bei 95.
    let candles = vec![
        Candle::new(1000, 100.0, 101.0, 99.0, 100.0, 500.0),
        Candle::new(2000, 99.0, 99.5, 94.0, 107.0, 600.0),
    ];
    let cfg = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        200_000.0,
        0.001,
        0.0,
        1.0,
    );
    let script = "stop_loss(95.0); if close() <= 105.0 { buy() }
";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg)
        .expect("backtest must complete");
    let expected = (95.0 - 100.0) - (95.0 + 100.0) * 0.001;
    assert_eq!(result.total_trades, 1);
    assert!((result.total_pnl - expected).abs() < 1e-6);
}

#[test]
fn test_take_profit_hits_and_closes_long_at_tp_price() {
    // Long bei 100, take_profit(110): naechste Kerze high 111 -> Close bei 110.
    let candles = vec![
        Candle::new(1000, 100.0, 101.0, 99.0, 100.0, 500.0),
        Candle::new(2000, 108.0, 111.0, 107.0, 112.0, 600.0),
    ];
    let cfg = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        200_000.0,
        0.001,
        0.0,
        1.0,
    );
    let script = "take_profit(110.0); if close() <= 105.0 { buy() }
";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg)
        .expect("backtest must complete");
    let expected = (110.0 - 100.0) - (110.0 + 100.0) * 0.001;
    assert_eq!(result.total_trades, 1);
    assert!((result.total_pnl - expected).abs() < 1e-6);
}

#[test]
fn test_stop_loss_hits_and_closes_short_at_stop_price() {
    // Short bei 100, stop_loss(105): naechste Kerze high 106 -> Close bei 105.
    let candles = vec![
        Candle::new(1000, 100.0, 101.0, 99.0, 100.0, 500.0),
        Candle::new(2000, 104.0, 106.0, 103.0, 97.0, 600.0),
    ];
    let cfg = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        200_000.0,
        0.001,
        0.0,
        1.0,
    );
    let script = "stop_loss(105.0); if close() > 99.5 { sell() }
";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg)
        .expect("backtest must complete");
    let expected = (100.0 - 105.0) - (105.0 + 100.0) * 0.001;
    assert_eq!(result.total_trades, 1);
    assert!((result.total_pnl - expected).abs() < 1e-6);
}

#[test]
fn test_trailing_stop_ratchets_up_and_closes_long() {
    // Long bei 100, trailing 5%: high 112 hebt den Stop auf 106.4,
    // Kerze mit low 105 loest ihn aus.
    let candles = vec![
        Candle::new(1000, 100.0, 101.0, 99.0, 100.0, 500.0),
        Candle::new(2000, 105.0, 112.0, 107.0, 110.0, 600.0),
        Candle::new(3000, 108.0, 109.0, 105.0, 106.0, 700.0),
    ];
    let cfg = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        200_000.0,
        0.001,
        0.0,
        1.0,
    );
    let script = "trailing_stop(0.05); if close() < 101.0 { buy() }
";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg)
        .expect("backtest must complete");
    let expected = (106.4 - 100.0) - (106.4 + 100.0) * 0.001;
    assert_eq!(result.total_trades, 1);
    assert!((result.total_pnl - expected).abs() < 1e-6);
}

#[test]
fn test_close_realizes_pnl_without_phantom_leg() {
    // Closing a long must realize PnL into the balance and remove the
    // position — it must NOT leave a phantom short leg behind (the old
    // double-counting bug: total_pnl != final_balance - initial_balance).
    let candles = vec![
        Candle::new(1000, 100.0, 101.0, 99.0, 100.0, 500.0),
        Candle::new(2000, 110.0, 111.0, 109.0, 110.0, 600.0),
        Candle::new(3000, 111.0, 112.0, 110.0, 111.0, 700.0),
    ];
    let cfg = BacktestConfig {
        script_path: PathBuf::from("unused"),
        data_path: PathBuf::from("unused"),
        initial_balance: 200_000.0,
        fee_rate: 0.001,
        slippage_bps: 0.0,
        symbol: "BTCUSDT".to_string(),
        quantity: 1.0,
    };
    // Long only: buy below 105, never sell (final close at last candle).
    let script = "if close() < 105 { buy() }
";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg)
        .expect("backtest must complete");
    assert_eq!(result.total_trades, 1);
    // Entry 100 + 0% slippage, exit 111, fees 0.001 each side.
    let expected = (111.0 - 100.0) - (111.0 + 100.0) * 0.001;
    assert!((result.total_pnl - expected).abs() < 1e-6);
    assert!((result.final_balance - (cfg.initial_balance + expected)).abs() < 1e-6);
}

#[test]
fn test_backtest_always_buy_script_rising_prices_positive_pnl() {
    // Arrange
    let script = "if close() > 0 { buy() }";
    let candles = helper_rising_candles();
    let config = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        10000.0,
        0.001,
        5.0,
        1.0,
    );

    // Act
    let result = run_backtest_with_script_and_candles(script, &candles, &config)
        .expect("backtest run should succeed");

    // Assert
    assert!(result.total_pnl > 0.0);
    assert_eq!(result.total_trades, 1);
    assert!((result.win_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_backtest_always_sell_script_rising_prices_negative_pnl() {
    // Arrange
    let script = "if close() > 0 { sell() }";
    let candles = helper_rising_candles();
    let config = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        10000.0,
        0.001,
        5.0,
        1.0,
    );

    // Act
    let result = run_backtest_with_script_and_candles(script, &candles, &config)
        .expect("backtest run should succeed");

    // Assert
    assert!(result.total_pnl < 0.0);
    assert_eq!(result.total_trades, 1);
    assert!((result.win_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn test_win_rate_computed_as_wins_divided_by_total_trades() {
    // Arrange
    let script = "if close() < 105 { buy() } else { if close() > 115 { sell() } }";
    let candles = vec![
        Candle::new(1000, 100.0, 105.0, 95.0, 100.0, 100.0),
        Candle::new(2000, 120.0, 125.0, 115.0, 120.0, 100.0),
        Candle::new(3000, 130.0, 135.0, 125.0, 130.0, 100.0),
        Candle::new(4000, 100.0, 105.0, 95.0, 100.0, 100.0),
        Candle::new(5000, 110.0, 115.0, 105.0, 110.0, 100.0),
    ];
    let config = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        10000.0,
        0.0,
        0.0,
        1.0,
    );

    // Act
    let result = run_backtest_with_script_and_candles(script, &candles, &config)
        .expect("backtest should succeed");

    // Assert: 3 echte Schluesse (Long c1, Short c3, Long finale), alle profitabel.
    // Vor dem Netting-Fix wurden Phantom-Gegenseiten als Trades mitgezaehlt (5).
    assert_eq!(result.total_trades, 3);
    assert!((result.win_rate - 1.0).abs() < f64::EPSILON);
    assert!((result.final_balance - (config.initial_balance + 50.0)).abs() < 1e-6);
}

/// T-287/T-292: eine offene Position ist kein Verlust.
///
/// `apply_buy_fill` zieht den Nennwert VOLL vom Guthaben ab. Wer die
/// Equity als `balance + unrealized_pnl` rechnet, sieht beim Einstieg
/// einen Einbruch um die halbe Kontogroesse und einen Drawdown, den es
/// nie gab - gemessen am 01.10.2026 an adx_macd/BTC-1h: 75,18 % statt
/// 8,15 %, bei unveraendertem PnL und unveraenderter Rundenzahl.
#[test]
fn test_drawdown_zaehlt_den_positionswert_mit() {
    let candles = vec![
        Candle::new(1000, 500.0, 505.0, 495.0, 500.0, 500.0),
        Candle::new(2000, 520.0, 525.0, 515.0, 520.0, 600.0),
        Candle::new(3000, 540.0, 545.0, 535.0, 540.0, 700.0),
        Candle::new(4000, 560.0, 565.0, 555.0, 560.0, 800.0),
    ];
    let cfg = BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        1000.0,
        0.0,
        0.0,
        1.0,
    );
    // Long bei 500 = halbes Konto, danach nur steigende Kurse.
    let script = "if close() < 510 { buy() }\n";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg)
        .expect("backtest must complete");
    assert_eq!(result.total_trades, 1);
    assert!((result.total_pnl - 60.0).abs() < 1e-6);
    assert_eq!(
        result.max_drawdown, 0.0,
        "steigende Kurse koennen keinen Drawdown ergeben - der Einbruch waere der Positionswert"
    );
}
