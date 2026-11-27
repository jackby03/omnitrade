//! Argument-Auswertung fuer Indikator-Funktionen des Skript-Evaluators.

use omnitrade_core::Candle;

use crate::ast::Expr;
use crate::error::ScriptError;
use crate::evaluator::Evaluator;

impl Evaluator {
    pub(super) fn eval_args(
        &mut self,
        func_name: &str,
        args: &[Expr],
        count: usize,
        candles: &[Candle],
    ) -> Result<Vec<f64>, ScriptError> {
        if args.len() != count {
            return Err(ScriptError::EvalError {
                message: format!("{func_name}() expects {count} arguments"),
            });
        }
        let mut out = Vec::with_capacity(count);
        for a in args {
            let v = self.eval_expr(a, candles)?;
            if v <= 0.0 || v.is_nan() {
                return Err(ScriptError::EvalError {
                    message: format!("invalid argument for {func_name}: {v}"),
                });
            }
            out.push(v);
        }
        Ok(out)
    }

    pub(super) fn eval_period_arg(
        &mut self,
        func_name: &str,
        args: &[Expr],
        candles: &[Candle],
    ) -> Result<usize, ScriptError> {
        if args.len() != 1 {
            return Err(ScriptError::EvalError {
                message: format!("{func_name}() expects 1 argument"),
            });
        }
        let period_f = self.eval_expr(&args[0], candles)?;
        if period_f <= 0.0 || period_f.is_nan() {
            return Err(ScriptError::EvalError {
                message: format!("invalid period for {func_name}: {period_f}"),
            });
        }
        Ok(period_f as usize)
    }
}
