//! Strategy DSL script evaluator for `omnitrade-script`.
//!
//! Evaluates AST statements and expressions against historical candle data
//! to emit trading signals (`Signal::Buy`, `Signal::Sell`, `Signal::Hold`).

use std::collections::HashMap;

use omnitrade_core::{Candle, Ema, Rsi, Sma};

use crate::ast::{BinaryOp, Expr, Signal, Stmt};
use crate::error::ScriptError;
use crate::extra;
#[path = "evaluator_args.rs"]
mod evaluator_args;
use crate::order_intent::{EvaluatorState, OrderIntent};

/// Prozentwerte fuer Stop/Take-Profit muessen zwischen 0 und 1 liegen (exklusiv).
fn check_pct(name: &str, pct: f64) -> Result<(), ScriptError> {
    if pct.is_finite() && pct > 0.0 && pct < 1.0 {
        Ok(())
    } else {
        Err(ScriptError::EvalError {
            message: format!(
                "{name} muss zwischen 0 und 1 liegen (z. B. 0.03 fuer 3 %), ist {pct}"
            ),
        })
    }
}

fn short(name: &str, candles: &[Candle]) -> ScriptError {
    ScriptError::InsufficientHistory {
        message: format!("insufficient candles ({}) for {name}", candles.len()),
    }
}

/// Evaluator for strategy scripts.
///
/// Maintains variable state across statement evaluations.
#[derive(Debug)]
pub struct Evaluator {
    /// Environment map storing variable bindings.
    pub variables: HashMap<String, f64>,
    /// Current order intent tracking TP/SL/Trailing.
    current_intent: OrderIntent,
    /// Variable bindings as they stood before the current candle; `prev()` reads them.
    previous_variables: HashMap<String, f64>,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl Evaluator {
    /// Creates a new `Evaluator` with empty variable bindings.
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            current_intent: OrderIntent::default(),
            previous_variables: HashMap::new(),
        }
    }

    /// Returns the current order intent.
    pub fn current_intent(&self) -> &OrderIntent {
        &self.current_intent
    }

    /// Evaluates a sequence of strategy statements against market candle data.
    ///
    /// # Errors
    /// Returns [`ScriptError::EvalError`] if `candles` is empty or runtime evaluation fails.
    pub fn evaluate(&mut self, stmts: &[Stmt], candles: &[Candle]) -> Result<Signal, ScriptError> {
        if candles.is_empty() {
            return Err(ScriptError::EvalError {
                message: "candles dataset cannot be empty".to_string(),
            });
        }

        self.begin_evaluation();
        // Alle Statements auswerten, damit stop_loss/take_profit/trailing_stop
        // auch hinter buy()/sell() gelesen werden. Das erste Signal gewinnt.
        let mut first = None;
        for stmt in stmts {
            if let Some(signal) = self.eval_stmt(stmt, candles)? {
                first.get_or_insert(signal);
            }
        }
        let signal = first.unwrap_or(Signal::Hold);
        self.current_intent.signal = signal;
        Ok(signal)
    }

    /// Setzt die Auswertung fuer eine neue Kerze zurueck: der Order-Intent beginnt
    /// leer, und die Variablen der vorherigen Kerze werden fuer `prev()` gesichert.
    fn begin_evaluation(&mut self) {
        self.current_intent = OrderIntent::default();
        self.previous_variables = self.variables.clone();
    }

    /// Evaluates statements and returns complete OrderIntent with risk parameters.
    ///
    /// Processes all statements (including risk parameters) before returning.
    /// Trailing stop triggers are evaluated after all statements execute.
    ///
    /// # Errors
    /// Returns [`ScriptError::EvalError`] if `candles` is empty or runtime evaluation fails.
    pub fn evaluate_extended(
        &mut self,
        stmts: &[Stmt],
        candles: &[Candle],
        state: &mut EvaluatorState,
    ) -> Result<OrderIntent, ScriptError> {
        if candles.is_empty() {
            return Err(ScriptError::EvalError {
                message: "candles dataset cannot be empty".to_string(),
            });
        }

        // Variablen aus dem Zustand der vorherigen Kerze uebernehmen (fuer prev()).
        self.variables = state.variables.clone();
        self.begin_evaluation();

        // Process all statements without breaking on first signal
        for stmt in stmts {
            self.eval_stmt(stmt, candles)?;
        }
        state.variables = self.variables.clone();

        // Check trailing stop if configured (after all statements processed)
        if let Some(trailing_pct) = self.current_intent.trailing_pct {
            let current_price = candles.last().map(|c| c.close).unwrap_or(0.0);
            if state.update_and_check_trailing(current_price, trailing_pct) {
                self.current_intent.signal = Signal::Sell;
            }
        }

        Ok(self.current_intent.clone())
    }

    fn eval_stmt(
        &mut self,
        stmt: &Stmt,
        candles: &[Candle],
    ) -> Result<Option<Signal>, ScriptError> {
        match stmt {
            Stmt::Assign { name, value } => {
                let val = self.eval_expr(value, candles)?;
                self.variables.insert(name.clone(), val);
                Ok(None)
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
            } => {
                let cond_val = self.eval_expr(condition, candles)?;
                if cond_val != 0.0 {
                    self.eval_block(then_block, candles)
                } else if let Some(else_stmts) = else_block {
                    self.eval_block(else_stmts, candles)
                } else {
                    Ok(None)
                }
            }
            Stmt::SignalBuy => {
                self.current_intent.signal = Signal::Buy;
                Ok(Some(Signal::Buy))
            }
            Stmt::SignalSell => {
                self.current_intent.signal = Signal::Sell;
                Ok(Some(Signal::Sell))
            }
            Stmt::StopLoss(expr) => {
                let price = self.eval_expr(expr, candles)?;
                self.current_intent.stop_loss = Some(price);
                Ok(None)
            }
            Stmt::TakeProfit(expr) => {
                let price = self.eval_expr(expr, candles)?;
                self.current_intent.take_profit = Some(price);
                Ok(None)
            }
            Stmt::StopLossPct(expr) => {
                let pct = self.eval_expr(expr, candles)?;
                check_pct("stop_loss_pct", pct)?;
                self.current_intent.stop_loss_pct = Some(pct);
                Ok(None)
            }
            Stmt::TakeProfitPct(expr) => {
                let pct = self.eval_expr(expr, candles)?;
                check_pct("take_profit_pct", pct)?;
                self.current_intent.take_profit_pct = Some(pct);
                Ok(None)
            }
            Stmt::TrailingStop(expr) => {
                let pct = self.eval_expr(expr, candles)?;
                self.current_intent.trailing_pct = Some(pct);
                Ok(None)
            }
            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr, candles)?;
                Ok(None)
            }
        }
    }

    /// Führt alle Statements eines Blocks aus und liefert das erste Signal zurück.
    /// Kein Abbruch beim ersten Signal, sonst gingen `stop_loss(..)`/`take_profit(..)`
    /// hinter `buy()` im selben Block verloren.
    fn eval_block(
        &mut self,
        block: &[Stmt],
        candles: &[Candle],
    ) -> Result<Option<Signal>, ScriptError> {
        let mut first = None;
        for s in block {
            if let Some(sig) = self.eval_stmt(s, candles)? {
                first.get_or_insert(sig);
            }
        }
        Ok(first)
    }

    /// Evaluates an AST expression to a 64-bit floating point value.
    ///
    /// # Errors
    /// Returns [`ScriptError`] if variables/functions are missing, or arithmetic fails.
    pub fn eval_expr(&mut self, expr: &Expr, candles: &[Candle]) -> Result<f64, ScriptError> {
        match expr {
            Expr::Number(n) => Ok(*n),
            Expr::Identifier(name) => self
                .variables
                .get(name)
                .copied()
                .ok_or_else(|| ScriptError::UndefinedVariable { name: name.clone() }),
            Expr::Negate(inner) => {
                let val = self.eval_expr(inner, candles)?;
                Ok(-val)
            }
            Expr::BinaryOp { left, op, right } => match op {
                BinaryOp::And => {
                    let l_val = self.eval_expr(left, candles)?;
                    if l_val == 0.0 {
                        Ok(0.0)
                    } else {
                        let r_val = self.eval_expr(right, candles)?;
                        Ok(if r_val != 0.0 { 1.0 } else { 0.0 })
                    }
                }
                BinaryOp::Or => {
                    let l_val = self.eval_expr(left, candles)?;
                    if l_val != 0.0 {
                        Ok(1.0)
                    } else {
                        let r_val = self.eval_expr(right, candles)?;
                        Ok(if r_val != 0.0 { 1.0 } else { 0.0 })
                    }
                }
                _ => {
                    let l_val = self.eval_expr(left, candles)?;
                    let r_val = self.eval_expr(right, candles)?;
                    match op {
                        BinaryOp::Add => Ok(l_val + r_val),
                        BinaryOp::Sub => Ok(l_val - r_val),
                        BinaryOp::Mul => Ok(l_val * r_val),
                        BinaryOp::Div => {
                            if r_val == 0.0 {
                                Err(ScriptError::EvalError {
                                    message: "division by zero".to_string(),
                                })
                            } else {
                                Ok(l_val / r_val)
                            }
                        }
                        BinaryOp::Gt => Ok(if l_val > r_val { 1.0 } else { 0.0 }),
                        BinaryOp::Lt => Ok(if l_val < r_val { 1.0 } else { 0.0 }),
                        BinaryOp::Gte => Ok(if l_val >= r_val { 1.0 } else { 0.0 }),
                        BinaryOp::Lte => Ok(if l_val <= r_val { 1.0 } else { 0.0 }),
                        BinaryOp::Eq => Ok(if l_val == r_val { 1.0 } else { 0.0 }),
                        BinaryOp::Neq => Ok(if l_val != r_val { 1.0 } else { 0.0 }),
                        BinaryOp::And | BinaryOp::Or => unreachable!(),
                    }
                }
            },
            Expr::FunctionCall { name, args } => self.eval_function_call(name, args, candles),
        }
    }

    fn eval_function_call(
        &mut self,
        name: &str,
        args: &[Expr],
        candles: &[Candle],
    ) -> Result<f64, ScriptError> {
        match name {
            "close" => {
                if !args.is_empty() {
                    return Err(ScriptError::EvalError {
                        message: "close() expects 0 arguments".to_string(),
                    });
                }
                candles
                    .last()
                    .map(|c| c.close)
                    .ok_or_else(|| ScriptError::EvalError {
                        message: "candles dataset cannot be empty".to_string(),
                    })
            }
            "volume" => {
                if !args.is_empty() {
                    return Err(ScriptError::EvalError {
                        message: "volume() expects 0 arguments".to_string(),
                    });
                }
                candles
                    .last()
                    .map(|c| c.volume)
                    .ok_or_else(|| ScriptError::EvalError {
                        message: "candles dataset cannot be empty".to_string(),
                    })
            }
            "sma" => {
                let period = self.eval_period_arg("sma", args, candles)?;
                let mut indicator = Sma::new(period)?;
                let mut res = None;
                for c in candles {
                    res = indicator.update(c.close);
                }
                res.ok_or_else(|| ScriptError::InsufficientHistory {
                    message: format!(
                        "insufficient candles ({}) for sma with period {period}",
                        candles.len()
                    ),
                })
            }
            "rsi" => {
                let period = self.eval_period_arg("rsi", args, candles)?;
                let mut indicator = Rsi::new(period)?;
                let mut res = None;
                for c in candles {
                    res = indicator.update(c.close);
                }
                res.ok_or_else(|| ScriptError::InsufficientHistory {
                    message: format!(
                        "insufficient candles ({}) for rsi with period {period}",
                        candles.len()
                    ),
                })
            }
            "ema" => {
                let period = self.eval_period_arg("ema", args, candles)?;
                let mut indicator = Ema::new(period)?;
                let mut res = None;
                for c in candles {
                    res = indicator.update(c.close);
                }
                res.ok_or_else(|| ScriptError::InsufficientHistory {
                    message: format!(
                        "insufficient candles ({}) for ema with period {period}",
                        candles.len()
                    ),
                })
            }
            // highest/lowest rechnen ueber die `period` Kerzen VOR der aktuellen
            // (exklusive) — Donchian/Breakout-Semantik: der aktuelle Close kann
            // sonst nie das Fenster-High uebersteigen (high >= close).
            "highest" => {
                let period = self.eval_period_arg("highest", args, candles)?;
                if candles.len() < period + 1 {
                    return Err(ScriptError::InsufficientHistory {
                        message: format!(
                            "insufficient candles ({}) for highest with period {period}",
                            candles.len()
                        ),
                    });
                }
                let start = candles.len() - 1 - period;
                let window = &candles[start..candles.len() - 1];
                Ok(window
                    .iter()
                    .map(|c| c.high)
                    .fold(f64::NEG_INFINITY, f64::max))
            }
            "lowest" => {
                let period = self.eval_period_arg("lowest", args, candles)?;
                if candles.len() < period + 1 {
                    return Err(ScriptError::InsufficientHistory {
                        message: format!(
                            "insufficient candles ({}) for lowest with period {period}",
                            candles.len()
                        ),
                    });
                }
                let start = candles.len() - 1 - period;
                let window = &candles[start..candles.len() - 1];
                Ok(window.iter().map(|c| c.low).fold(f64::INFINITY, f64::min))
            }
            "atr" => {
                let period = self.eval_period_arg("atr", args, candles)?;
                if candles.len() < period + 1 {
                    return Err(ScriptError::InsufficientHistory {
                        message: format!(
                            "insufficient candles ({}) for atr with period {period}",
                            candles.len()
                        ),
                    });
                }
                let start = candles.len() - period;
                let mut sum = 0.0;
                for i in start..candles.len() {
                    let c = &candles[i];
                    let prev_close = candles[i - 1].close;
                    let tr = (c.high - c.low)
                        .max((c.high - prev_close).abs())
                        .max((c.low - prev_close).abs());
                    sum += tr;
                }
                Ok(sum / period as f64)
            }
            // prev(expr): der Ausdruck auf der Kerzenreihe ohne die letzte Kerze.
            // So laesst sich ein Kreuzen schreiben: a > b && prev(a) <= prev(b).
            "prev" => {
                if args.len() != 1 {
                    return Err(ScriptError::EvalError {
                        message: "prev() expects 1 argument".to_string(),
                    });
                }
                if candles.len() < 2 {
                    return Err(short(name, candles));
                }
                // Der Ausdruck wird mit den Variablen der vorherigen Kerze berechnet,
                // sonst liefe `prev(x)` auf dem aktuellen Wert von `x`.
                let current =
                    std::mem::replace(&mut self.variables, self.previous_variables.clone());
                let result = self.eval_expr(&args[0], &candles[..candles.len() - 1]);
                self.variables = current;
                result
            }
            "macd" | "macd_signal" | "macd_hist" => {
                let a = self.eval_args(name, args, 3, candles)?;
                let (fast, slow, sig) = (a[0] as usize, a[1] as usize, a[2] as usize);
                let (line, signal, hist) =
                    extra::macd(candles, fast, slow, sig).ok_or_else(|| short(name, candles))?;
                Ok(match name {
                    "macd" => line,
                    "macd_signal" => signal,
                    _ => hist,
                })
            }
            "bb_lower" | "bb_mid" | "bb_upper" => {
                let a = self.eval_args(name, args, 2, candles)?;
                let (lo, mid, hi) = extra::bollinger(candles, a[0] as usize, a[1])
                    .ok_or_else(|| short(name, candles))?;
                Ok(match name {
                    "bb_lower" => lo,
                    "bb_mid" => mid,
                    _ => hi,
                })
            }
            "stoch_k" => {
                let a = self.eval_args(name, args, 1, candles)?;
                extra::stoch_k(candles, a[0] as usize).ok_or_else(|| short(name, candles))
            }
            "adx" | "plus_di" | "minus_di" => {
                let a = self.eval_args(name, args, 1, candles)?;
                let (adx, pdi, ndi) =
                    extra::adx(candles, a[0] as usize).ok_or_else(|| short(name, candles))?;
                Ok(match name {
                    "adx" => adx,
                    "plus_di" => pdi,
                    _ => ndi,
                })
            }
            _ => Err(ScriptError::UndefinedFunction {
                name: name.to_string(),
            }),
        }
    }
}

#[cfg(test)]
#[path = "evaluator_tests.rs"]
mod tests;
