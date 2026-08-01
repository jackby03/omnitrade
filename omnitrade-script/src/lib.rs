//! # omnitrade-script
//!
//! Strategy DSL interpreter with AST definitions, lexer, parser, and evaluator.

pub mod ast;
pub mod error;
pub mod lexer;
pub mod token;

pub use ast::{BinaryOp, Expr, Signal, Stmt};
pub use error::ScriptError;
pub use lexer::tokenize;
pub use token::Token;
