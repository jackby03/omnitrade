//! # omnitrade-script
//!
//! Strategy DSL interpreter with AST definitions, lexer, parser, and evaluator.
//!
//! This crate provides tokenization, parsing, and execution of strategy scripts
//! against historical candle data to produce trading signals (`Signal::Buy`, `Signal::Sell`, `Signal::Hold`).

pub mod ast;
pub mod error;
pub mod evaluator;
pub mod extra;
pub mod grid;
pub mod lexer;
pub mod order_intent;
pub mod parser;
pub mod token;

pub use ast::{BinaryOp, Expr, Signal, Stmt};
pub use error::ScriptError;
pub use evaluator::Evaluator;
pub use grid::GridConfig;
pub use lexer::tokenize;
pub use order_intent::{EvaluatorState, OrderIntent};
pub use parser::parse;
pub use token::Token;

/// Compiles and evaluates a strategy DSL script against a sequence of candles.
///
/// # Errors
///
/// Returns [`ScriptError`] if tokenization fails, parsing fails, or runtime
/// evaluation encounters an error.
pub fn compile_and_run(
    script: &str,
    candles: &[omnitrade_core::Candle],
) -> Result<Signal, ScriptError> {
    let tokens = tokenize(script)?;
    let stmts = parse(&tokens)?;
    let mut evaluator = Evaluator::new();
    evaluator.evaluate(&stmts, candles)
}

/// Compiles and evaluates a strategy DSL script with extended order-intent handling.
///
/// Returns complete [`OrderIntent`] with signal, stop-loss, take-profit, and trailing-stop parameters.
/// Maintains persistent state for trailing-stop calculations across candle sequences.
///
/// # Errors
///
/// Returns [`ScriptError`] if tokenization fails, parsing fails, or runtime
/// evaluation encounters an error.
pub fn compile_and_run_extended(
    script: &str,
    candles: &[omnitrade_core::Candle],
    state: &mut EvaluatorState,
) -> Result<OrderIntent, ScriptError> {
    let tokens = tokenize(script)?;
    let stmts = parse(&tokens)?;
    let mut evaluator = Evaluator::new();
    evaluator.evaluate_extended(&stmts, candles, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnitrade_core::Candle;

    fn helper_candles(prices: &[f64]) -> Vec<Candle> {
        prices
            .iter()
            .enumerate()
            .map(|(i, &p)| Candle::new(i as u64 * 1000, p, p + 1.0, p - 1.0, p, 100.0))
            .collect()
    }

    #[test]
    fn test_compile_and_run_sell_signal() {
        // Arrange
        let script = "if close() > 50000 { sell() }";
        let candles = helper_candles(&[51000.0]);

        // Act
        let result = compile_and_run(script, &candles);

        // Assert
        let signal = result.expect("compilation and execution should succeed");
        assert_eq!(signal, Signal::Sell);
    }

    #[test]
    fn test_compile_and_run_buy_signal() {
        // Arrange
        let script = "if close() < 40000 { buy() }";
        let candles = helper_candles(&[35000.0]);

        // Act
        let result = compile_and_run(script, &candles);

        // Assert
        let signal = result.expect("compilation and execution should succeed");
        assert_eq!(signal, Signal::Buy);
    }

    #[test]
    fn test_compile_and_run_hold_signal() {
        // Arrange
        let script = "if close() > 50000 { sell() }";
        let candles = helper_candles(&[45000.0]);

        // Act
        let result = compile_and_run(script, &candles);

        // Assert
        let signal = result.expect("compilation and execution should succeed");
        assert_eq!(signal, Signal::Hold);
    }

    #[test]
    fn test_compile_and_run_lex_error() {
        // Arrange
        let script = "if @ invalid { sell() }";
        let candles = helper_candles(&[50000.0]);

        // Act
        let result = compile_and_run(script, &candles);

        // Assert
        assert!(matches!(result, Err(ScriptError::LexError { .. })));
    }

    #[test]
    fn test_compile_and_run_parse_error() {
        // Arrange
        let script = "if (close() > 50000 { sell() }";
        let candles = helper_candles(&[50000.0]);

        // Act
        let result = compile_and_run(script, &candles);

        // Assert
        assert!(matches!(result, Err(ScriptError::ParseError { .. })));
    }

    #[test]
    fn test_compile_and_run_eval_error() {
        // Arrange
        let script = "if close() > 50000 { sell() }";
        let candles: Vec<Candle> = vec![];

        // Act
        let result = compile_and_run(script, &candles);

        // Assert
        assert!(matches!(result, Err(ScriptError::EvalError { .. })));
    }

    #[test]
    fn test_stop_loss_pct_sets_relative_stop_on_intent() {
        // Arrange
        let script = "buy(); stop_loss_pct(0.03)";
        let candles = helper_candles(&[110.0]);
        let mut state = EvaluatorState::new(110.0);

        // Act
        let intent = compile_and_run_extended(script, &candles, &mut state);

        // Assert
        let order = intent.expect("extended evaluation should succeed");
        assert_eq!(order.stop_loss_pct, Some(0.03));
        assert_eq!(order.stop_loss, None);
    }

    #[test]
    fn test_take_profit_pct_rejects_values_outside_zero_one() {
        // Arrange
        let script = "buy(); take_profit_pct(1.5)";
        let candles = helper_candles(&[110.0]);
        let mut state = EvaluatorState::new(110.0);

        // Act
        let result = compile_and_run_extended(script, &candles, &mut state);

        // Assert
        assert!(result.is_err());
    }
    #[test]
    fn test_compile_and_run_extended_with_stop_loss() {
        // Arrange
        let script = "buy(); stop_loss(100.0); take_profit(120.0)";
        let candles = helper_candles(&[110.0]);
        let mut state = EvaluatorState::new(110.0);

        // Act
        let intent = compile_and_run_extended(script, &candles, &mut state);

        // Assert
        let order = intent.expect("extended evaluation should succeed");
        assert_eq!(order.signal, Signal::Buy);
        assert_eq!(order.stop_loss, Some(100.0));
        assert_eq!(order.take_profit, Some(120.0));
    }

    #[test]
    fn test_compile_and_run_extended_with_trailing_stop() {
        // Arrange
        let script = "buy(); trailing_stop(0.05)";
        let prices = vec![100.0, 105.0, 110.0];
        let candles = helper_candles(&prices);
        let mut state = EvaluatorState::new(100.0);

        // Act
        let intent = compile_and_run_extended(script, &candles, &mut state);

        // Assert
        let order = intent.expect("extended evaluation should succeed");
        assert_eq!(order.signal, Signal::Buy);
        assert_eq!(order.trailing_pct, Some(0.05));
    }

    #[test]
    fn test_backward_compat_compile_and_run() {
        // Arrange: Verify old API still works unchanged
        let script = "if close() > 100 { buy() }";
        let candles = helper_candles(&[110.0]);

        // Act
        let signal = compile_and_run(script, &candles);

        // Assert
        assert_eq!(signal, Ok(Signal::Buy));
    }
}
