//! Parser for strategy DSL scripts.

use crate::ast::{BinaryOp, Expr, Stmt};
use crate::error::ScriptError;
use crate::token::Token;

/// Parses a slice of [`Token`]s into a vector of Abstract Syntax Tree [`Stmt`]s.
///
/// # Errors
/// Returns [`ScriptError::ParseError`] if invalid syntax, unexpected tokens, or missing delimiters are encountered.
pub fn parse(tokens: &[Token]) -> Result<Vec<Stmt>, ScriptError> {
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse_program(&mut self) -> Result<Vec<Stmt>, ScriptError> {
        let mut stmts = Vec::new();
        while !self.is_at_end() {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.index).unwrap_or(&Token::Eof)
    }

    fn peek_nth(&self, n: usize) -> &Token {
        self.tokens.get(self.index + n).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.index += 1;
        }
        self.tokens.get(self.index - 1).unwrap_or(&Token::Eof)
    }

    fn consume(&mut self, expected: &Token, message: &str) -> Result<(), ScriptError> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            Err(ScriptError::ParseError {
                token_index: self.index,
                message: message.to_string(),
            })
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let stmt = match self.peek() {
            Token::If => self.parse_if_stmt()?,
            Token::Buy => self.parse_signal_stmt(true)?,
            Token::Sell => self.parse_signal_stmt(false)?,
            Token::Identifier(_) if self.peek_nth(1) == &Token::Assign => {
                self.parse_assign_stmt()?
            }
            _ => Stmt::ExprStmt(self.parse_expr()?),
        };
        if self.peek() == &Token::Semicolon {
            self.advance();
        }
        Ok(stmt)
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ScriptError> {
        self.advance();
        let condition = self.parse_expr()?;
        self.consume(&Token::LBrace, "expected '{' after if condition")?;
        let then_block = self.parse_block()?;
        self.consume(&Token::RBrace, "expected '}' after if block")?;
        let else_block = if self.peek() == &Token::Else {
            self.advance();
            self.consume(&Token::LBrace, "expected '{' after else")?;
            let block = self.parse_block()?;
            self.consume(&Token::RBrace, "expected '}' after else block")?;
            Some(block)
        } else {
            None
        };
        Ok(Stmt::If {
            condition,
            then_block,
            else_block,
        })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ScriptError> {
        let mut stmts = Vec::new();
        while !self.is_at_end() && self.peek() != &Token::RBrace {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_signal_stmt(&mut self, is_buy: bool) -> Result<Stmt, ScriptError> {
        self.advance();
        if self.peek() == &Token::LParen {
            self.advance();
            let msg = if is_buy {
                "expected ')' after 'buy'"
            } else {
                "expected ')' after 'sell'"
            };
            self.consume(&Token::RParen, msg)?;
        }
        Ok(if is_buy {
            Stmt::SignalBuy
        } else {
            Stmt::SignalSell
        })
    }

    fn parse_assign_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let name = if let Token::Identifier(id) = self.peek() {
            let n = id.clone();
            self.advance();
            n
        } else {
            return Err(ScriptError::ParseError {
                token_index: self.index,
                message: "expected identifier in assignment".to_string(),
            });
        };
        self.consume(&Token::Assign, "expected '=' in assignment")?;
        let value = self.parse_expr()?;
        Ok(Stmt::Assign { name, value })
    }

    fn parse_expr(&mut self) -> Result<Expr, ScriptError> {
        self.parse_or()
    }

    fn parse_binary<F>(
        &mut self,
        next: F,
        matcher: fn(&Token) -> Option<BinaryOp>,
    ) -> Result<Expr, ScriptError>
    where
        F: Fn(&mut Self) -> Result<Expr, ScriptError>,
    {
        let mut left = next(self)?;
        while let Some(op) = matcher(self.peek()) {
            self.advance();
            let right = next(self)?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, ScriptError> {
        self.parse_binary(Self::parse_and, |t| {
            (t == &Token::Or).then_some(BinaryOp::Or)
        })
    }

    fn parse_and(&mut self) -> Result<Expr, ScriptError> {
        self.parse_binary(Self::parse_equality, |t| {
            (t == &Token::And).then_some(BinaryOp::And)
        })
    }

    fn parse_equality(&mut self) -> Result<Expr, ScriptError> {
        self.parse_binary(Self::parse_comparison, |t| match t {
            Token::EqEq => Some(BinaryOp::Eq),
            Token::BangEq => Some(BinaryOp::Neq),
            _ => None,
        })
    }

    fn parse_comparison(&mut self) -> Result<Expr, ScriptError> {
        self.parse_binary(Self::parse_add_sub, |t| match t {
            Token::Lt => Some(BinaryOp::Lt),
            Token::Gt => Some(BinaryOp::Gt),
            Token::Lte => Some(BinaryOp::Lte),
            Token::Gte => Some(BinaryOp::Gte),
            _ => None,
        })
    }

    fn parse_add_sub(&mut self) -> Result<Expr, ScriptError> {
        self.parse_binary(Self::parse_mul_div, |t| match t {
            Token::Plus => Some(BinaryOp::Add),
            Token::Minus => Some(BinaryOp::Sub),
            _ => None,
        })
    }

    fn parse_mul_div(&mut self) -> Result<Expr, ScriptError> {
        self.parse_binary(Self::parse_unary, |t| match t {
            Token::Star => Some(BinaryOp::Mul),
            Token::Slash => Some(BinaryOp::Div),
            _ => None,
        })
    }

    fn parse_unary(&mut self) -> Result<Expr, ScriptError> {
        if self.peek() == &Token::Minus {
            self.advance();
            Ok(Expr::Negate(Box::new(self.parse_unary()?)))
        } else if self.peek() == &Token::Plus {
            self.advance();
            self.parse_unary()
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ScriptError> {
        let tok_idx = self.index;
        match self.peek() {
            Token::Number(val) => {
                let v = *val;
                self.advance();
                Ok(Expr::Number(v))
            }
            Token::Identifier(id) => {
                let name = id.clone();
                self.advance();
                if self.peek() == &Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != &Token::RParen {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.peek() == &Token::Comma {
                                self.advance();
                                if self.peek() == &Token::RParen {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    self.consume(&Token::RParen, "expected ')' after function arguments")?;
                    Ok(Expr::FunctionCall { name, args })
                } else {
                    Ok(Expr::Identifier(name))
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.consume(&Token::RParen, "expected ')' after expression")?;
                Ok(expr)
            }
            token => Err(ScriptError::ParseError {
                token_index: tok_idx,
                message: format!("unexpected token: {token:?}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn test_parse_assign_statement() {
        // Arrange
        let tokens = tokenize("x = sma(20);").expect("tokenization should succeed");

        // Act
        let ast = parse(&tokens).expect("parsing should succeed");

        // Assert
        let expected = vec![Stmt::Assign {
            name: "x".to_string(),
            value: Expr::FunctionCall {
                name: "sma".to_string(),
                args: vec![Expr::Number(20.0)],
            },
        }];
        assert_eq!(ast, expected);
    }

    #[test]
    fn test_parse_if_buy_statement() {
        // Arrange
        let tokens = tokenize("if rsi(14) < 30 { buy() }").expect("tokenization should succeed");

        // Act
        let ast = parse(&tokens).expect("parsing should succeed");

        // Assert
        let expected = vec![Stmt::If {
            condition: Expr::BinaryOp {
                left: Box::new(Expr::FunctionCall {
                    name: "rsi".to_string(),
                    args: vec![Expr::Number(14.0)],
                }),
                op: BinaryOp::Lt,
                right: Box::new(Expr::Number(30.0)),
            },
            then_block: vec![Stmt::SignalBuy],
            else_block: None,
        }];
        assert_eq!(ast, expected);
    }

    #[test]
    fn test_parse_operator_precedence() {
        // Arrange
        let tokens = tokenize("a + b * c").expect("tokenization should succeed");

        // Act
        let ast = parse(&tokens).expect("parsing should succeed");

        // Assert
        let expected = vec![Stmt::ExprStmt(Expr::BinaryOp {
            left: Box::new(Expr::Identifier("a".to_string())),
            op: BinaryOp::Add,
            right: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier("b".to_string())),
                op: BinaryOp::Mul,
                right: Box::new(Expr::Identifier("c".to_string())),
            }),
        })];
        assert_eq!(ast, expected);
    }

    #[test]
    fn test_parse_if_else_branches() {
        // Arrange
        let tokens =
            tokenize("if x > 0 { buy() } else { sell() }").expect("tokenization should succeed");

        // Act
        let ast = parse(&tokens).expect("parsing should succeed");

        // Assert
        let expected = vec![Stmt::If {
            condition: Expr::BinaryOp {
                left: Box::new(Expr::Identifier("x".to_string())),
                op: BinaryOp::Gt,
                right: Box::new(Expr::Number(0.0)),
            },
            then_block: vec![Stmt::SignalBuy],
            else_block: Some(vec![Stmt::SignalSell]),
        }];
        assert_eq!(ast, expected);
    }

    #[test]
    fn test_parse_missing_closing_brace() {
        // Arrange
        let tokens = tokenize("if x > 0 { buy()").expect("tokenization should succeed");

        // Act
        let result = parse(&tokens);

        // Assert
        assert!(matches!(
            result,
            Err(ScriptError::ParseError { message, .. }) if message.contains("expected '}'")
        ));
    }

    #[test]
    fn test_parse_unexpected_token() {
        // Arrange
        let tokens = tokenize("x = ;").expect("tokenization should succeed");

        // Act
        let result = parse(&tokens);

        // Assert
        assert_eq!(
            result,
            Err(ScriptError::ParseError {
                token_index: 2,
                message: "unexpected token: Semicolon".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_logical_and_or_precedence() {
        // Arrange
        let tokens = tokenize("a || b && c").expect("tokenization should succeed");

        // Act
        let ast = parse(&tokens).expect("parsing should succeed");

        // Assert
        let expected = vec![Stmt::ExprStmt(Expr::BinaryOp {
            left: Box::new(Expr::Identifier("a".to_string())),
            op: BinaryOp::Or,
            right: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier("b".to_string())),
                op: BinaryOp::And,
                right: Box::new(Expr::Identifier("c".to_string())),
            }),
        })];
        assert_eq!(ast, expected);
    }

    #[test]
    fn test_parse_buy_and_sell_without_parens() {
        // Arrange
        let tokens = tokenize("buy; sell").expect("tokenization should succeed");

        // Act
        let ast = parse(&tokens).expect("parsing should succeed");

        // Assert
        let expected = vec![Stmt::SignalBuy, Stmt::SignalSell];
        assert_eq!(ast, expected);
    }

    #[test]
    fn test_parse_unary_negation() {
        // Arrange
        let tokens = tokenize("-price + 10").expect("tokenization should succeed");

        // Act
        let ast = parse(&tokens).expect("parsing should succeed");

        // Assert
        let expected = vec![Stmt::ExprStmt(Expr::BinaryOp {
            left: Box::new(Expr::Negate(Box::new(Expr::Identifier(
                "price".to_string(),
            )))),
            op: BinaryOp::Add,
            right: Box::new(Expr::Number(10.0)),
        })];
        assert_eq!(ast, expected);
    }
}
