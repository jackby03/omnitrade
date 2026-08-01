//! Token definitions for strategy DSL scripts.

/// Tokens produced during lexical analysis of strategy scripts.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    If,
    Else,
    Buy,
    Sell,

    // Literals
    Number(f64),
    Identifier(String),

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Lt,
    Gt,
    Lte,
    Gte,
    EqEq,
    BangEq,
    And,
    Or,
    Assign,

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semicolon,

    // End of File
    Eof,
}
