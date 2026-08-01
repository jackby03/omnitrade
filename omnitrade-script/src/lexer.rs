//! Lexer for strategy DSL scripts.

use crate::error::ScriptError;
use crate::token::Token;

/// Tokenizes a strategy script into a list of [`Token`]s.
///
/// # Errors
/// Returns [`ScriptError::LexError`] if an unrecognized character or invalid numeric format is encountered.
pub fn tokenize(input: &str) -> Result<Vec<Token>, ScriptError> {
    let mut lexer = Lexer::new(input);
    lexer.run()
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn run(&mut self) -> Result<Vec<Token>, ScriptError> {
        let mut tokens = Vec::new();

        while let Some(ch) = self.peek_char() {
            match ch {
                ' ' | '\t' | '\r' | '\n' => self.advance_char(),
                '/' => {
                    if self.peek_next_char() == Some('/') {
                        while let Some(c) = self.peek_char() {
                            self.advance_char();
                            if c == '\n' {
                                break;
                            }
                        }
                    } else {
                        self.advance_char();
                        tokens.push(Token::Slash);
                    }
                }
                '<' => tokens.push(self.match_two('=', Token::Lte, Token::Lt)),
                '>' => tokens.push(self.match_two('=', Token::Gte, Token::Gt)),
                '=' => tokens.push(self.match_two('=', Token::EqEq, Token::Assign)),
                '!' => {
                    self.advance_char();
                    if self.peek_char() == Some('=') {
                        self.advance_char();
                        tokens.push(Token::BangEq);
                    } else {
                        return Err(ScriptError::LexError {
                            position: self.pos - 1,
                            message: "unexpected character '!'".to_string(),
                        });
                    }
                }
                '&' => tokens.push(self.match_required('&', Token::And, "expected '&&'")?),
                '|' => tokens.push(self.match_required('|', Token::Or, "expected '||'")?),
                '+' => self.single_token(&mut tokens, Token::Plus),
                '-' => self.single_token(&mut tokens, Token::Minus),
                '*' => self.single_token(&mut tokens, Token::Star),
                '(' => self.single_token(&mut tokens, Token::LParen),
                ')' => self.single_token(&mut tokens, Token::RParen),
                '{' => self.single_token(&mut tokens, Token::LBrace),
                '}' => self.single_token(&mut tokens, Token::RBrace),
                ',' => self.single_token(&mut tokens, Token::Comma),
                ';' => self.single_token(&mut tokens, Token::Semicolon),
                c if c.is_ascii_digit()
                    || (c == '.'
                        && self
                            .peek_next_char()
                            .is_some_and(|next| next.is_ascii_digit())) =>
                {
                    tokens.push(self.read_number()?);
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    tokens.push(self.read_identifier_or_keyword());
                }
                c => {
                    let err_pos = self.pos;
                    self.advance_char();
                    return Err(ScriptError::LexError {
                        position: err_pos,
                        message: format!("unexpected character '{c}'"),
                    });
                }
            }
        }

        tokens.push(Token::Eof);
        Ok(tokens)
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn peek_next_char(&self) -> Option<char> {
        let mut chars = self.input[self.pos..].chars();
        chars.next()?;
        chars.next()
    }

    fn advance_char(&mut self) {
        if let Some(c) = self.peek_char() {
            self.pos += c.len_utf8();
        }
    }

    fn single_token(&mut self, tokens: &mut Vec<Token>, tok: Token) {
        self.advance_char();
        tokens.push(tok);
    }

    fn match_two(&mut self, expected: char, matched: Token, default: Token) -> Token {
        self.advance_char();
        if self.peek_char() == Some(expected) {
            self.advance_char();
            matched
        } else {
            default
        }
    }

    fn match_required(
        &mut self,
        expected: char,
        tok: Token,
        err_msg: &str,
    ) -> Result<Token, ScriptError> {
        let start_pos = self.pos;
        self.advance_char();
        if self.peek_char() == Some(expected) {
            self.advance_char();
            Ok(tok)
        } else {
            Err(ScriptError::LexError {
                position: start_pos,
                message: format!("unexpected character, {err_msg}"),
            })
        }
    }

    fn read_number(&mut self) -> Result<Token, ScriptError> {
        let start = self.pos;
        let mut has_decimal = false;

        while let Some(c) = self.peek_char() {
            if c.is_ascii_digit() {
                self.advance_char();
            } else if c == '.' && !has_decimal {
                has_decimal = true;
                self.advance_char();
            } else {
                break;
            }
        }

        let num_str = &self.input[start..self.pos];
        num_str
            .parse::<f64>()
            .map(Token::Number)
            .map_err(|_| ScriptError::LexError {
                position: start,
                message: format!("invalid numeric literal '{num_str}'"),
            })
    }

    fn read_identifier_or_keyword(&mut self) -> Token {
        let start = self.pos;
        while let Some(c) = self.peek_char() {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.advance_char();
            } else {
                break;
            }
        }

        let ident = &self.input[start..self.pos];
        match ident {
            "if" => Token::If,
            "else" => Token::Else,
            "buy" => Token::Buy,
            "sell" => Token::Sell,
            _ => Token::Identifier(ident.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_if_rsi_buy() {
        // Arrange
        let input = "if rsi(14) < 30 { buy() }";

        // Act
        let tokens = tokenize(input).expect("tokenization should succeed");

        // Assert
        let expected = vec![
            Token::If,
            Token::Identifier("rsi".to_string()),
            Token::LParen,
            Token::Number(14.0),
            Token::RParen,
            Token::Lt,
            Token::Number(30.0),
            Token::LBrace,
            Token::Buy,
            Token::LParen,
            Token::RParen,
            Token::RBrace,
            Token::Eof,
        ];
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_tokenize_gte_operator() {
        // Arrange
        let input = "sma(20) >= ema(50)";

        // Act
        let tokens = tokenize(input).expect("tokenization should succeed");

        // Assert
        let expected = vec![
            Token::Identifier("sma".to_string()),
            Token::LParen,
            Token::Number(20.0),
            Token::RParen,
            Token::Gte,
            Token::Identifier("ema".to_string()),
            Token::LParen,
            Token::Number(50.0),
            Token::RParen,
            Token::Eof,
        ];
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_tokenize_decimal_numbers() {
        // Arrange
        let input = "price <= 100.5 and val == .5";

        // Act
        let tokens = tokenize(input).expect("tokenization should succeed");

        // Assert
        let expected = vec![
            Token::Identifier("price".to_string()),
            Token::Lte,
            Token::Number(100.5),
            Token::Identifier("and".to_string()),
            Token::Identifier("val".to_string()),
            Token::EqEq,
            Token::Number(0.5),
            Token::Eof,
        ];
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_tokenize_skips_comments() {
        // Arrange
        let input = "// comment\nbuy()";

        // Act
        let tokens = tokenize(input).expect("tokenization should succeed");

        // Assert
        let expected = vec![Token::Buy, Token::LParen, Token::RParen, Token::Eof];
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_tokenize_unrecognized_character() {
        // Arrange
        let input = "@";

        // Act
        let result = tokenize(input);

        // Assert
        assert_eq!(
            result,
            Err(ScriptError::LexError {
                position: 0,
                message: "unexpected character '@'".to_string(),
            })
        );
    }

    #[test]
    fn test_tokenize_empty_input() {
        // Arrange
        let input = "";

        // Act
        let tokens = tokenize(input).expect("tokenization should succeed");

        // Assert
        assert_eq!(tokens, vec![Token::Eof]);
    }

    #[test]
    fn test_tokenize_all_operators_and_delimiters() {
        // Arrange
        let input = "+ - * / < > <= >= == != && || = ( ) { } , ; else sell";

        // Act
        let tokens = tokenize(input).expect("tokenization should succeed");

        // Assert
        let expected = vec![
            Token::Plus,
            Token::Minus,
            Token::Star,
            Token::Slash,
            Token::Lt,
            Token::Gt,
            Token::Lte,
            Token::Gte,
            Token::EqEq,
            Token::BangEq,
            Token::And,
            Token::Or,
            Token::Assign,
            Token::LParen,
            Token::RParen,
            Token::LBrace,
            Token::RBrace,
            Token::Comma,
            Token::Semicolon,
            Token::Else,
            Token::Sell,
            Token::Eof,
        ];
        assert_eq!(tokens, expected);
    }
}
