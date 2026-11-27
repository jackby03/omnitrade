//! Additional indicators for the script language: MACD, Bollinger Bands,
//! Stochastic %K and ADX. Pure functions over a candle slice; each returns the
//! value at the last candle, or `None` when there is not enough history.

use omnitrade_core::{Candle, Ema};

/// MACD line, signal line and histogram at the last candle.
pub fn macd(
    candles: &[Candle],
    fast: usize,
    slow: usize,
    signal: usize,
) -> Option<(f64, f64, f64)> {
    let mut f = Ema::new(fast).ok()?;
    let mut s = Ema::new(slow).ok()?;
    let mut sig = Ema::new(signal).ok()?;
    let mut last = None;
    for c in candles {
        let fv = f.update(c.close);
        let sv = s.update(c.close);
        if let (Some(fv), Some(sv)) = (fv, sv) {
            let line = fv - sv;
            last = sig.update(line).map(|sg| (line, sg, line - sg));
        }
    }
    last
}

/// Bollinger Bands (lower, middle, upper) over the last `period` closes.
pub fn bollinger(candles: &[Candle], period: usize, k: f64) -> Option<(f64, f64, f64)> {
    if period == 0 || candles.len() < period {
        return None;
    }
    let w = &candles[candles.len() - period..];
    let mean = w.iter().map(|c| c.close).sum::<f64>() / period as f64;
    let var = w.iter().map(|c| (c.close - mean).powi(2)).sum::<f64>() / period as f64;
    let dev = var.sqrt() * k;
    Some((mean - dev, mean, mean + dev))
}

/// Stochastic %K (0..100): close position within the last `period` candles.
pub fn stoch_k(candles: &[Candle], period: usize) -> Option<f64> {
    if period == 0 || candles.len() < period {
        return None;
    }
    let w = &candles[candles.len() - period..];
    let hi = w.iter().map(|c| c.high).fold(f64::NEG_INFINITY, f64::max);
    let lo = w.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);
    let close = candles.last()?.close;
    Some(if hi > lo {
        (close - lo) / (hi - lo) * 100.0
    } else {
        50.0
    })
}

/// Wilder ADX with the directional indicators: (adx, +DI, -DI) at the last candle.
pub fn adx(candles: &[Candle], period: usize) -> Option<(f64, f64, f64)> {
    if period == 0 || candles.len() < 2 * period + 1 {
        return None;
    }
    let p = period as f64;
    let (mut tr_s, mut pdm_s, mut ndm_s) = (0.0, 0.0, 0.0);
    let mut dx_first = Vec::new();
    let mut adx_v: Option<f64> = None;
    let (mut pdi, mut ndi) = (0.0, 0.0);
    for i in 1..candles.len() {
        let (c, pv) = (&candles[i], &candles[i - 1]);
        let up = c.high - pv.high;
        let down = pv.low - c.low;
        let pdm = if up > down && up > 0.0 { up } else { 0.0 };
        let ndm = if down > up && down > 0.0 { down } else { 0.0 };
        let tr = (c.high - c.low)
            .max((c.high - pv.close).abs())
            .max((c.low - pv.close).abs());
        if i <= period {
            tr_s += tr;
            pdm_s += pdm;
            ndm_s += ndm;
        } else {
            tr_s = tr_s - tr_s / p + tr;
            pdm_s = pdm_s - pdm_s / p + pdm;
            ndm_s = ndm_s - ndm_s / p + ndm;
        }
        if i >= period && tr_s > 0.0 {
            pdi = 100.0 * pdm_s / tr_s;
            ndi = 100.0 * ndm_s / tr_s;
            let sum = pdi + ndi;
            let dx = if sum > 0.0 {
                100.0 * (pdi - ndi).abs() / sum
            } else {
                0.0
            };
            match adx_v {
                Some(a) => adx_v = Some((a * (p - 1.0) + dx) / p),
                None => {
                    dx_first.push(dx);
                    if dx_first.len() == period {
                        adx_v = Some(dx_first.iter().sum::<f64>() / p);
                    }
                }
            }
        }
    }
    adx_v.map(|a| (a, pdi, ndi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(prices: &[f64]) -> Vec<Candle> {
        prices
            .iter()
            .enumerate()
            .map(|(i, &p)| Candle::new(i as u64 * 1000, p, p + 1.0, p - 1.0, p, 100.0))
            .collect()
    }

    #[test]
    fn test_macd_positive_in_uptrend_and_none_when_short() {
        let up: Vec<f64> = (0..80).map(|i| 100.0 + i as f64).collect();
        let (line, _, _) = macd(&series(&up), 12, 26, 9).unwrap();
        assert!(line > 0.0);
        assert!(macd(&series(&up[..20]), 12, 26, 9).is_none());
    }

    #[test]
    fn test_bollinger_flat_series_has_zero_width() {
        let (lo, mid, hi) = bollinger(&series(&[10.0; 30]), 20, 2.0).unwrap();
        assert_eq!((lo, mid, hi), (10.0, 10.0, 10.0));
    }

    #[test]
    fn test_bollinger_bands_are_symmetric() {
        let p: Vec<f64> = (0..30).map(|i| 100.0 + (i % 5) as f64).collect();
        let (lo, mid, hi) = bollinger(&series(&p), 20, 2.0).unwrap();
        assert!(lo < mid && mid < hi);
        assert!((mid - lo - (hi - mid)).abs() < 1e-9);
    }

    #[test]
    fn test_stoch_k_extremes() {
        let up: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        assert!(stoch_k(&series(&up), 14).unwrap() > 80.0);
        let down: Vec<f64> = (0..20).map(|i| 100.0 - i as f64).collect();
        assert!(stoch_k(&series(&down), 14).unwrap() < 20.0);
        assert!(stoch_k(&series(&up[..5]), 14).is_none());
    }

    #[test]
    fn test_adx_high_in_trend_low_in_chop() {
        let up: Vec<f64> = (0..80).map(|i| 100.0 + 2.0 * i as f64).collect();
        let (a, pdi, ndi) = adx(&series(&up), 14).unwrap();
        assert!(a > 40.0 && pdi > ndi);
        let chop: Vec<f64> = (0..80).map(|i| 100.0 + (i % 2) as f64).collect();
        let (a2, _, _) = adx(&series(&chop), 14).unwrap();
        assert!(a2 < 20.0);
        assert!(adx(&series(&up[..20]), 14).is_none());
    }
}
