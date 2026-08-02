//! # omnitrade-script
//!
//! Strategy DSL interpreter with AST definitions, lexer, parser, and evaluator.
//!
//! This crate provides tokenization, parsing, and execution of strategy scripts
//! against historical candle data to produce trading signals (`Signal::Buy`, `Signal::Sell`, `Signal::Hold`).

pub mod ast;
pub mod error;
pub mod evaluator;
pub mod lexer;
pub mod parser;
pub mod token;

pub use ast::{BinaryOp, Expr, Signal, Stmt};
pub use error::ScriptError;
pub use evaluator::Evaluator;
pub use lexer::tokenize;
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
}
