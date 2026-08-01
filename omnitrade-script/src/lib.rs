//! # omnitrade-script
//!
//! Strategy DSL interpreter with AST definitions, lexer, parser, and evaluator.

pub mod ast;
pub mod error;

pub use ast::{BinaryOp, Expr, Signal, Stmt};
pub use error::ScriptError;
