use super::*;
use crate::lexer::tokenize;
use crate::parser::parse;

fn helper_candles(prices: &[f64]) -> Vec<Candle> {
    prices
        .iter()
        .enumerate()
        .map(|(i, &p)| Candle::new(i as u64 * 1000, p, p + 1.0, p - 1.0, p, 100.0))
        .collect()
}

#[test]
fn test_evaluate_rsi_buy_signal() {
    // Arrange: create script and candle data where RSI < 30
    let code = "if rsi(14) < 30 { buy() }";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");
    let mut prices = vec![100.0; 5];
    for i in 1..=15 {
        prices.push(100.0 - (i as f64 * 4.0));
    }
    let candles = helper_candles(&prices);
    let mut evaluator = Evaluator::new();

    // Act
    let signal = evaluator
        .evaluate(&stmts, &candles)
        .expect("evaluation should succeed");

    // Assert
    assert_eq!(signal, Signal::Buy);
}

#[test]
fn test_evaluate_sma_cross_signal() {
    // Arrange
    let code = "if sma(10) > sma(20) { buy() } else { sell() }";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");

    let prices: Vec<f64> = (1..=30).map(|i| i as f64 * 10.0).collect();
    let candles = helper_candles(&prices);
    let mut evaluator = Evaluator::new();

    // Act
    let signal = evaluator
        .evaluate(&stmts, &candles)
        .expect("evaluation should succeed");

    // Assert
    assert_eq!(signal, Signal::Buy);
}

#[test]
fn test_evaluate_variable_binding() {
    // Arrange
    let code = "x = close(); if x > 50000 { sell() }";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");
    let candles = vec![Candle::new(1000, 51000.0, 52000.0, 50000.0, 51000.0, 10.0)];
    let mut evaluator = Evaluator::new();

    // Act
    let signal = evaluator
        .evaluate(&stmts, &candles)
        .expect("evaluation should succeed");

    // Assert
    assert_eq!(signal, Signal::Sell);
    assert_eq!(evaluator.variables.get("x"), Some(&51000.0));
}

#[test]
fn test_evaluate_undefined_function() {
    // Arrange
    let code = "foo(1)";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");
    let candles = helper_candles(&[100.0]);
    let mut evaluator = Evaluator::new();

    // Act
    let result = evaluator.evaluate(&stmts, &candles);

    // Assert
    assert_eq!(
        result,
        Err(ScriptError::UndefinedFunction {
            name: "foo".to_string()
        })
    );
}

#[test]
fn test_evaluate_division_by_zero() {
    // Arrange
    let code = "x = 10 / 0";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");
    let candles = helper_candles(&[100.0]);
    let mut evaluator = Evaluator::new();

    // Act
    let result = evaluator.evaluate(&stmts, &candles);

    // Assert
    assert!(matches!(result, Err(ScriptError::EvalError { .. })));
}

#[test]
fn test_evaluate_empty_candle_slice() {
    // Arrange
    let code = "buy()";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");
    let candles: Vec<Candle> = vec![];
    let mut evaluator = Evaluator::new();

    // Act
    let result = evaluator.evaluate(&stmts, &candles);

    // Assert
    assert!(matches!(result, Err(ScriptError::EvalError { .. })));
}

#[test]
fn test_evaluate_short_circuit_and_or() {
    // Arrange: 0 && undefined_var should not fail with UndefinedVariable
    let code1 = "if 0 && undefined_var { buy() }";
    let code2 = "if 1 || undefined_var { buy() }";
    let tokens1 = tokenize(code1).expect("tokenization should succeed");
    let tokens2 = tokenize(code2).expect("tokenization should succeed");
    let stmts1 = parse(&tokens1).expect("parsing should succeed");
    let stmts2 = parse(&tokens2).expect("parsing should succeed");
    let candles = helper_candles(&[100.0]);
    let mut evaluator = Evaluator::new();

    // Act & Assert
    let sig1 = evaluator
        .evaluate(&stmts1, &candles)
        .expect("short circuit AND");
    assert_eq!(sig1, Signal::Hold);

    let sig2 = evaluator
        .evaluate(&stmts2, &candles)
        .expect("short circuit OR");
    assert_eq!(sig2, Signal::Buy);
}

#[test]
fn test_evaluate_extended_with_stop_loss() {
    // Arrange
    let code = "buy(); stop_loss(90.0); take_profit(110.0)";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");
    let candles = helper_candles(&[100.0]);
    let mut evaluator = Evaluator::new();
    let mut state = EvaluatorState::new(100.0);

    // Act
    let intent = evaluator
        .evaluate_extended(&stmts, &candles, &mut state)
        .expect("extended evaluation should succeed");

    // Assert
    assert_eq!(intent.signal, Signal::Buy);
    assert_eq!(intent.stop_loss, Some(90.0));
    assert_eq!(intent.take_profit, Some(110.0));
}

#[test]
fn test_stop_loss_and_take_profit_after_signal_inside_if_block() {
    // Arrange
    let code = "if close() > 0 { buy(); stop_loss(90.0); take_profit(110.0) }";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");
    let candles = helper_candles(&[100.0]);
    let mut evaluator = Evaluator::new();
    let mut state = EvaluatorState::new(100.0);

    // Act
    let intent = evaluator
        .evaluate_extended(&stmts, &candles, &mut state)
        .expect("extended evaluation should succeed");

    // Assert
    assert_eq!(intent.signal, Signal::Buy);
    assert_eq!(intent.stop_loss, Some(90.0));
    assert_eq!(intent.take_profit, Some(110.0));
}

#[test]
fn test_evaluate_extended_with_trailing_stop() {
    // Arrange
    let code = "buy(); trailing_stop(0.05)";
    let tokens = tokenize(code).expect("tokenization should succeed");
    let stmts = parse(&tokens).expect("parsing should succeed");
    let prices = vec![100.0, 105.0, 110.0, 109.0];
    let candles = helper_candles(&prices);
    let mut evaluator = Evaluator::new();
    let mut state = EvaluatorState::new(100.0);

    // Act
    let intent = evaluator
        .evaluate_extended(&stmts, &candles, &mut state)
        .expect("extended evaluation should succeed");

    // Assert: current price 109 < highest 110 * (1 - 0.05) = 104.5, so no trigger
    assert_eq!(intent.signal, Signal::Buy);
    assert_eq!(intent.trailing_pct, Some(0.05));
}

#[test]
fn test_evaluate_highest_function() {
    // highs = p+1: 11,13,12,16,14; highest(3) ueber letzte 3 = 16
    let candles = helper_candles(&[10.0, 12.0, 11.0, 15.0, 13.0]);
    let tokens = tokenize("if highest(3) < 20 { buy() }").expect("tokenize");
    let stmts = parse(&tokens).expect("parse");
    let mut evaluator = Evaluator::new();
    let sig = evaluator
        .evaluate(&stmts, &candles)
        .expect("evaluation should succeed");
    assert_eq!(sig, Signal::Buy);
}

#[test]
fn test_evaluate_lowest_function() {
    // lows = p-1: 9,11,10,14,12; lowest(3) = 10
    let candles = helper_candles(&[10.0, 12.0, 11.0, 15.0, 13.0]);
    let tokens = tokenize("if lowest(3) > 5 { buy() }").expect("tokenize");
    let stmts = parse(&tokens).expect("parse");
    let mut evaluator = Evaluator::new();
    let sig = evaluator
        .evaluate(&stmts, &candles)
        .expect("evaluation should succeed");
    assert_eq!(sig, Signal::Buy);
}

#[test]
fn test_evaluate_atr_function() {
    // closes 10,11,12; TR1=2, TR2=2 -> atr(2)=2
    let candles = helper_candles(&[10.0, 11.0, 12.0]);
    let tokens = tokenize("if atr(2) > 1 { buy() }").expect("tokenize");
    let stmts = parse(&tokens).expect("parse");
    let mut evaluator = Evaluator::new();
    let sig = evaluator
        .evaluate(&stmts, &candles)
        .expect("evaluation should succeed");
    assert_eq!(sig, Signal::Buy);
}

#[test]
fn test_evaluate_insufficient_highest() {
    let candles = helper_candles(&[1.0, 2.0]);
    let tokens = tokenize("if highest(5) > 0 { buy() }").expect("tokenize");
    let stmts = parse(&tokens).expect("parse");
    let mut evaluator = Evaluator::new();
    assert!(matches!(
        evaluator.evaluate(&stmts, &candles),
        Err(ScriptError::InsufficientHistory { .. })
    ));
}

#[test]
fn test_new_indicators_callable_from_script() {
    let prices: Vec<f64> = (0..80).map(|i| 100.0 + 2.0 * i as f64).collect();
    let candles = helper_candles(&prices);
    let code = "if macd_hist(12, 26, 9) > -1000 && adx(14) > 40 && plus_di(14) > minus_di(14) && stoch_k(14) > 80 && close() > bb_mid(20, 2) && bb_upper(20, 2) > bb_lower(20, 2) && macd(12, 26, 9) > macd_signal(12, 26, 9) - 1000 { buy() }";
    let stmts = parse(&tokenize(code).unwrap()).unwrap();
    let mut evaluator = Evaluator::new();
    assert_eq!(evaluator.evaluate(&stmts, &candles).unwrap(), Signal::Buy);
    let bad = parse(&tokenize("if adx(14, 3) > 1 { buy() }").unwrap()).unwrap();
    assert!(Evaluator::new().evaluate(&bad, &candles).is_err());
}

#[test]
fn test_prev_detects_crossover() {
    // Kurs 10,10,10,20: nur auf der letzten Kerze kreuzt close ueber 15.
    let candles = helper_candles(&[10.0, 10.0, 10.0, 20.0]);
    let code = "if close() > 15 && prev(close()) <= 15 { buy() }";
    let stmts = parse(&tokenize(code).unwrap()).unwrap();
    assert_eq!(
        Evaluator::new().evaluate(&stmts, &candles).unwrap(),
        Signal::Buy
    );
    // Eine Kerze spaeter ist der Kurs schon drueber: kein Kreuzen mehr.
    let candles = helper_candles(&[10.0, 10.0, 20.0, 20.0]);
    assert_eq!(
        Evaluator::new().evaluate(&stmts, &candles).unwrap(),
        Signal::Hold
    );
    let bad = parse(&tokenize("if prev(1, 2) > 0 { buy() }").unwrap()).unwrap();
    assert!(Evaluator::new().evaluate(&bad, &candles).is_err());
}

#[test]
fn test_prev_reads_variable_from_previous_candle() {
    // x = close(): auf der letzten Kerze ist x = 20, auf der vorherigen 10.
    let candles = helper_candles(&[10.0, 10.0, 10.0, 20.0]);
    let stmts =
        parse(&tokenize("x = close(); if x > 15 && prev(x) <= 15 { buy() }").unwrap()).unwrap();
    let mut evaluator = Evaluator::new();
    let mut last = Signal::Hold;
    for i in 0..candles.len() {
        last = evaluator.evaluate(&stmts, &candles[..=i]).unwrap();
    }
    assert_eq!(last, Signal::Buy);
}

#[test]
fn test_directives_after_signal_are_collected() {
    let candles = helper_candles(&[100.0, 100.0]);
    let stmts = parse(&tokenize("buy(); stop_loss(95); take_profit(110)").unwrap()).unwrap();
    let mut evaluator = Evaluator::new();
    assert_eq!(evaluator.evaluate(&stmts, &candles).unwrap(), Signal::Buy);
    assert_eq!(evaluator.current_intent().stop_loss, Some(95.0));
    assert_eq!(evaluator.current_intent().take_profit, Some(110.0));
}

#[test]
fn test_intent_is_reset_per_evaluation() {
    let candles = helper_candles(&[100.0, 100.0]);
    let with_stop = parse(&tokenize("stop_loss(95); buy()").unwrap()).unwrap();
    let without = parse(&tokenize("buy()").unwrap()).unwrap();
    let mut evaluator = Evaluator::new();
    evaluator.evaluate(&with_stop, &candles).unwrap();
    assert_eq!(evaluator.current_intent().stop_loss, Some(95.0));
    evaluator.evaluate(&without, &candles).unwrap();
    assert_eq!(evaluator.current_intent().stop_loss, None);
}
