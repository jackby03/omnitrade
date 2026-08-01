//! Domain-specific error types for `omnitrade-script`.
//!
//! All errors produced by lexing, parsing, evaluating, or executing strategy scripts
//! are represented by [`ScriptError`].

use omnitrade_core::CoreError;
use thiserror::Error;

/// Errors produced during strategy script processing.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ScriptError {
    /// Lexical analysis error occurred at a specific position.
    #[error("lexical error at position {position}: {message}")]
    LexError {
        /// Character offset in source script where error occurred.
        position: usize,
        /// Detailed description of lexical error.
        message: String,
    },

    /// Syntax parsing error occurred at a specific token index.
    #[error("parse error at token index {token_index}: {message}")]
    ParseError {
        /// Token index where parse failure occurred.
        token_index: usize,
        /// Detailed description of parse error.
        message: String,
    },

    /// Runtime evaluation error.
    #[error("evaluation error: {message}")]
    EvalError {
        /// Detailed description of evaluation error.
        message: String,
    },

    /// An undefined variable was referenced.
    #[error("undefined variable: {name}")]
    UndefinedVariable {
        /// Name of the missing variable.
        name: String,
    },

    /// An undefined function was called.
    #[error("undefined function: {name}")]
    UndefinedFunction {
        /// Name of the missing function.
        name: String,
    },

    /// Type mismatch encountered during evaluation.
    #[error("type error: expected {expected}, got {got}")]
    TypeError {
        /// Expected type description.
        expected: String,
        /// Actual type description received.
        got: String,
    },

    /// Error originating from `omnitrade-core`.
    #[error("core error: {0}")]
    Core(#[from] CoreError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_formatting() {
        // Arrange
        let lex_err = ScriptError::LexError {
            position: 12,
            message: "unexpected character '@'".to_string(),
        };
        let parse_err = ScriptError::ParseError {
            token_index: 5,
            message: "expected token ';'".to_string(),
        };
        let eval_err = ScriptError::EvalError {
            message: "division by zero".to_string(),
        };
        let var_err = ScriptError::UndefinedVariable {
            name: "ma_period".to_string(),
        };
        let fn_err = ScriptError::UndefinedFunction {
            name: "custom_indicator".to_string(),
        };
        let type_err = ScriptError::TypeError {
            expected: "number".to_string(),
            got: "bool".to_string(),
        };

        // Act & Assert
        assert_eq!(
            lex_err.to_string(),
            "lexical error at position 12: unexpected character '@'"
        );
        assert_eq!(
            parse_err.to_string(),
            "parse error at token index 5: expected token ';'"
        );
        assert_eq!(eval_err.to_string(), "evaluation error: division by zero");
        assert_eq!(var_err.to_string(), "undefined variable: ma_period");
        assert_eq!(fn_err.to_string(), "undefined function: custom_indicator");
        assert_eq!(
            type_err.to_string(),
            "type error: expected number, got bool"
        );
    }

    #[test]
    fn test_core_error_conversion() {
        // Arrange
        let core_err = CoreError::InvalidPeriod(0);

        // Act
        let script_err: ScriptError = core_err.into();

        // Assert
        assert_eq!(script_err, ScriptError::Core(CoreError::InvalidPeriod(0)));
        assert_eq!(
            script_err.to_string(),
            "core error: invalid period: 0 (must be greater than zero)"
        );
    }

    #[test]
    fn test_error_equality() {
        // Arrange
        let err1 = ScriptError::UndefinedVariable {
            name: "x".to_string(),
        };
        let err2 = ScriptError::UndefinedVariable {
            name: "x".to_string(),
        };
        let err3 = ScriptError::UndefinedVariable {
            name: "y".to_string(),
        };

        // Act & Assert
        assert_eq!(err1, err2);
        assert_ne!(err1, err3);
    }
}
