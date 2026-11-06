use super::*;

fn cfg(slippage_bps: f64, fee_rate: f64) -> BacktestConfig {
    BacktestConfig::new(
        PathBuf::from("dummy.omni"),
        PathBuf::from("dummy.csv"),
        1000.0,
        fee_rate,
        slippage_bps,
        1.0,
    )
}

#[test]
fn gap_below_stop_exits_at_open_not_at_stop() {
    // Long bei 100, Stop 95. Die naechste Kerze oeffnet bei 90: Ausfuehrung zum Open.
    let candles = vec![
        Candle::new(1000, 100.0, 100.0, 100.0, 100.0, 500.0),
        Candle::new(2000, 90.0, 90.0, 90.0, 90.0, 600.0),
    ];
    let script = "stop_loss(95.0); if close() == 100.0 { buy() }\n";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg(0.0, 0.001))
        .expect("backtest must complete");
    let expected = (90.0 - 100.0) - (90.0 + 100.0) * 0.001;
    assert_eq!(result.total_trades, 1);
    assert!((result.total_pnl - expected).abs() < 1e-6);
}

#[test]
fn opening_stop_hit_wins_over_later_take_profit() {
    // Long bei 100, Stop 95, TP 110. Kerze oeffnet bei 95 und laeuft danach hoch:
    // der Stop am Open greift vor dem Take-Profit.
    let candles = vec![
        Candle::new(1000, 100.0, 100.0, 100.0, 100.0, 500.0),
        Candle::new(2000, 95.0, 111.0, 94.0, 105.0, 600.0),
    ];
    let script = "stop_loss(95.0); take_profit(110.0); if close() == 100.0 { buy() }\n";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg(0.0, 0.001))
        .expect("backtest must complete");
    let expected = (95.0 - 100.0) - (95.0 + 100.0) * 0.001;
    assert!((result.total_pnl - expected).abs() < 1e-6);
}

#[test]
fn trailing_stop_is_armed_at_entry() {
    // Long bei 100, Trailing 10 %: Stop ab Eroeffnung bei 90, ohne vorheriges Hoch.
    let candles = vec![
        Candle::new(1000, 100.0, 100.0, 100.0, 100.0, 500.0),
        Candle::new(2000, 100.0, 100.0, 85.0, 85.0, 600.0),
    ];
    let script = "trailing_stop(0.10); if close() == 100.0 { buy() }\n";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg(0.0, 0.001))
        .expect("backtest must complete");
    let expected = (90.0 - 100.0) - (90.0 + 100.0) * 0.001;
    assert_eq!(result.total_trades, 1);
    assert!((result.total_pnl - expected).abs() < 1e-6);
}

#[test]
fn closing_position_pays_exit_slippage() {
    // Entry mit 100 bps Slippage bei 101, Schluss zum Kurs 100 mit 100 bps bei 99.
    let candles = vec![
        Candle::new(1000, 100.0, 100.0, 100.0, 100.0, 500.0),
        Candle::new(2000, 100.0, 100.0, 100.0, 100.0, 500.0),
    ];
    let script = "if close() < 101.0 { buy() }\n";
    let result = run_backtest_with_script_and_candles(script, &candles, &cfg(100.0, 0.0))
        .expect("backtest must complete");
    assert!((result.final_balance - 998.0).abs() < 1e-6);
}

#[test]
fn genuine_evaluation_error_aborts_instead_of_skipping() {
    let candles = vec![Candle::new(1000, 100.0, 100.0, 100.0, 100.0, 500.0)];
    let script = "if 1 / 0 > 0 { buy() }\n";
    assert!(run_backtest_with_script_and_candles(script, &candles, &cfg(0.0, 0.0)).is_err());
}
