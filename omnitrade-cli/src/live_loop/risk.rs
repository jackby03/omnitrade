//! Risiko- und Eigenkapital-Helfer der Live-Schleife: Symbolzerlegung, Bewertung,
//! Liquidationsabstand und Positionsdeckel.

use omnitrade_core::Side;
use omnitrade_exchange::{Account, PositionSnapshot};

use super::{KNOWN_QUOTES, MAINNET_HARD_CAP};

/// Zerlegt ein Börsensymbol in (Basis, Quote): `BTC-USDT`, `BTC-USDT-SWAP`,
/// `BTC/USD` oder `BTCUSDT`, datierte Futures `BTC-USD-261030` und EEA-XPERPs
/// `SOL-USD_UM_XPERP-310404` (Settlement-Währung = Quote). Das Suffix `-SWAP`
/// (Perpetual) bezeichnet dasselbe Paar.
pub(crate) fn split_symbol(symbol: &str) -> Option<(String, String)> {
    let upper = symbol.to_uppercase();
    let pair = upper.strip_suffix("-SWAP").unwrap_or(upper.as_str());
    // XPERP: BASE-QUOTE_UM_XPERP-YYMMDD (EEA-Perpetual)
    if let Some(idx) = pair.find("_UM_XPERP-") {
        let head = &pair[..idx];
        let expiry = &pair[idx + "_UM_XPERP-".len()..];
        if let Some((base, quote)) = head.split_once('-') {
            if !base.is_empty()
                && !quote.is_empty()
                && quote.chars().all(|c| c.is_ascii_alphanumeric())
                && expiry.len() == 6
                && expiry.chars().all(|c| c.is_ascii_digit())
            {
                return Some((base.to_string(), quote.to_string()));
            }
        }
        return None;
    }
    // BASE-QUOTE-YYMMDD (datiertes Future, z. B. BTC-USD-261030)
    if pair.split('-').count() == 3 {
        let parts: Vec<&str> = pair.split('-').collect();
        let (base, quote, expiry) = (parts[0], parts[1], parts[2]);
        if !base.is_empty()
            && !quote.is_empty()
            && quote.chars().all(|c| c.is_ascii_alphanumeric())
            && expiry.len() == 6
            && expiry.chars().all(|c| c.is_ascii_digit())
        {
            return Some((base.to_string(), quote.to_string()));
        }
        return None;
    }
    if let Some((base, quote)) = pair.split_once(['-', '/']) {
        return (!base.is_empty() && !quote.is_empty() && !quote.contains(['-', '/']))
            .then(|| (base.to_string(), quote.to_string()));
    }
    KNOWN_QUOTES.iter().find_map(|quote| {
        pair.strip_suffix(quote)
            .filter(|base| !base.is_empty())
            .map(|base| (base.to_string(), (*quote).to_string()))
    })
}

/// Eigenkapital des Handelspaars: Quote-Bestand plus Basis-Bestand zum aktuellen Preis
/// (jeweils frei + gesperrt). Nur das Quote-Guthaben zu zählen ließe jeden Kauf wie
/// einen Verlust in Höhe des Kaufpreises aussehen.
pub(crate) fn pair_equity(account: &Account, base: &str, quote: &str, price: f64) -> f64 {
    let total = |asset: &str| {
        account
            .balances
            .get(asset)
            .map_or(0.0, |b| b.free + b.locked)
    };
    total(quote) + total(base) * price
}

/// Eigenkapital für das Verlustlimit: Kontostände plus unrealisierter PnL offener
/// Perpetuals. Ohne den PnL erscheint ein offener Verlust erst beim Schließen.
pub(crate) fn account_equity(
    account: &Account,
    base: &str,
    quote: &str,
    price: f64,
    position: &PositionSnapshot,
) -> f64 {
    pair_equity(account, base, quote, price) + position.unrealized_pnl
}

/// Waehrung, in der die Konto-Equity bewertet wird.
///
/// Das Symbol nennt bei XPERP `USD`, das Konto fuehrt aber USDC (die Abrechnungs-
/// waehrung des Kontrakts). Wer stur die Quote nimmt, findet keinen Bestand und
/// bewertet das Konto mit 0 — der RiskGuard misst dann einen Drawdown von
/// Millionen Prozent und lehnt jeden Auftrag ab.
///
/// Regel: die Quote gilt, solange das Konto sie traegt. Traegt es sie nicht und
/// kennt das Instrument eine Abrechnungswaehrung mit Bestand, gilt diese. Sonst
/// bleibt es bei der Quote (unveraendertes Verhalten, fail open zur alten Regel).
pub(crate) fn bewertungs_waehrung<'a>(
    account: &Account,
    quote: &'a str,
    settle: Option<&'a str>,
) -> &'a str {
    let hat_bestand = |ccy: &str| {
        account
            .balances
            .get(ccy)
            .is_some_and(|b| b.free + b.locked > 0.0)
    };
    if hat_bestand(quote) {
        return quote;
    }
    match settle {
        Some(ccy) if hat_bestand(ccy) => ccy,
        _ => quote,
    }
}

/// Mindestabstand Markpreis zu Liquidationspreis (relativ zum Markpreis), bevor eine
/// Position weiter vergrößert werden darf. Bei 10x isolated liegt der Abstand direkt
/// nach dem Einstieg bei rund 9 %; 5 % lässt nur noch wenig Spielraum.
pub(crate) const MIN_LIQUIDATION_DISTANCE: f64 = 0.05;

/// Lehnt Orders ab, die eine offene Perpetual-Position vergrößern, wenn sie der
/// Liquidation zu nahe ist. Reduzierende Orders bleiben erlaubt, damit man aussteigen
/// kann. Fehlen die Preise bei offener Position, wird abgelehnt (fail closed).
pub(crate) fn check_liquidation_distance(
    position: &PositionSnapshot,
    side: Side,
) -> Result<(), String> {
    if !position.is_perpetual || position.base_qty == 0.0 {
        return Ok(());
    }
    let long = position.base_qty > 0.0;
    let increases = (side == Side::Buy) == long;
    if !increases {
        return Ok(());
    }
    let (Some(liq), Some(mark)) = (position.liq_price, position.mark_price) else {
        return Err("Liquidationspreis oder Markpreis fehlt bei offener Position".to_string());
    };
    if !mark.is_finite() || mark <= 0.0 {
        return Err(format!("ungültiger Markpreis {mark}"));
    }
    let distance = (mark - liq).abs() / mark;
    if distance < MIN_LIQUIDATION_DISTANCE {
        return Err(format!(
            "Abstand zur Liquidation {:.2} % unter Mindestwert {:.0} %",
            distance * 100.0,
            MIN_LIQUIDATION_DISTANCE * 100.0
        ));
    }
    Ok(())
}

/// Menge, auf die der Positionsdeckel wirkt: bei Perpetuals die Positionsmenge,
/// bei Spot der Bestand. Sonst wäre der Deckel bei Perpetuals immer 0 und wirkungslos.
pub(crate) fn held_base(account: &Account, base: &str, position: &PositionSnapshot) -> f64 {
    if position.is_perpetual {
        return position.base_qty;
    }
    account
        .balances
        .get(base)
        .map_or(0.0, |b| b.free + b.locked)
}

/// Long-only-Bremse, UNABHÄNGIG vom Positionsdeckel.
///
/// Ohne `--max-notional` gibt es sonst gar keine Long-only-Regel: ein Sell bei
/// flacher Position würde eine Kurzposition eröffnen. Die Baseline ist der
/// Bestand beim ERSTEN geprüften Auftrag; nur die in diesem Lauf gekaufte Menge
/// ist verkaufbar, Vorbestand wird nie angefasst.
#[derive(Debug, Default)]
pub(crate) struct LongOnlyGuard {
    start_base: Option<f64>,
}

impl LongOnlyGuard {
    pub(crate) fn new() -> Self {
        Self { start_base: None }
    }

    /// `held` ist die aktuelle NETTO-Position (`held_base`).
    pub(crate) fn check(&mut self, side: Side, quantity: f64, held: f64) -> Result<(), String> {
        let start = *self.start_base.get_or_insert(held);
        let run_base = held - start;
        let after = match side {
            Side::Buy => run_base + quantity,
            Side::Sell => run_base - quantity,
        };
        // Toleranz 1 % der Ordermenge: Spot-Gebühren in der Basis-Währung lassen
        // den Bestand leicht unter der gekauften Menge liegen.
        if after < -(quantity * 0.01) {
            return Err(format!(
                "Long-only-Lauf: Verkauf {quantity} übersteigt die in diesem Lauf \
                 gekaufte Menge {run_base} (Kurzposition nur bei erlaubten Shorts)"
            ));
        }
        Ok(())
    }
}

/// Prüft einen Positionsdeckel für die Position, die ein Lauf selbst aufbaut.
///
/// Die Laufposition wird SIGNIERT relativ zum Startbestand gemessen
/// (`held - start`, Buy addiert, Sell subtrahiert). Der Deckel begrenzt
/// `|Laufposition| × Preis` in BEIDE Richtungen. Ohne `allow_shorts` ist der
/// Lauf long-only: Die Baseline (Startbestand) darf nicht unterschritten
/// werden, Bestände aus der Zeit vor dem Lauf zählen nicht dazu und werden
/// nie verkauft. Mit `allow_shorts` (Haussatz C.2: erst nach belegtem
/// Long-Lauf) öffnet ein Sell bei flacher Position einen Short (OKX net
/// mode, signierte Position).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExposureLimit {
    cap: f64,
    start_base: Option<f64>,
    allow_shorts: bool,
}

impl ExposureLimit {
    pub(crate) fn new(cap: f64) -> Self {
        Self {
            cap,
            start_base: None,
            allow_shorts: false,
        }
    }

    /// Uebernimmt den Bestand beim Start als Laufposition (Baseline 0 statt
    /// Bestand beim ersten Auftrag). Fuer den Neustart eines Laufs, dessen
    /// Position an der Boerse offen blieb: sonst bliebe sie "fremd" und der
    /// Lauf koennte sie nie schliessen. Der Deckel zaehlt sie dann mit.
    #[must_use]
    pub(crate) fn adopt_existing(mut self, adopt: bool) -> Self {
        if adopt {
            self.start_base = Some(0.0);
        }
        self
    }

    /// Erlaubt Kurzpositionen: Sell-Signale dürfen die Baseline unterschreiten
    /// (bis zum Deckel). Standard ist long-only.
    #[must_use]
    pub(crate) fn allow_shorts(mut self, allow: bool) -> Self {
        self.allow_shorts = allow;
        self
    }

    /// `held_base` ist die aktuelle NETTO-Position (signiert; bei Perpetuals
    /// die Positionsgröße, bei Spot der Basis-Bestand).
    pub(crate) fn check(
        &mut self,
        side: Side,
        quantity: f64,
        price: f64,
        held: f64,
    ) -> Result<(), String> {
        let start = *self.start_base.get_or_insert(held);
        let run_base = held - start;
        let after = match side {
            Side::Buy => run_base + quantity,
            Side::Sell => run_base - quantity,
        };
        if after.abs() * price > self.cap + 1e-9 {
            return Err(format!(
                "Positionsdeckel überschritten: {} > {:.2} (Laufposition {after:+.4})",
                after.abs() * price,
                self.cap
            ));
        }
        // Toleranz 1 % der Ordermenge: Spot-Gebühren in der Basis-Währung lassen
        // den Bestand leicht unter der gekauften Menge liegen.
        if !self.allow_shorts && after < -(quantity * 0.01) {
            return Err(format!(
                "Long-only-Lauf: Verkauf {quantity} übersteigt die in diesem Lauf \
                 gekaufte Menge {run_base} (Kurzposition nur bei erlaubten Shorts)"
            ));
        }
        Ok(())
    }
}

/// `true`, wenn der Deckel für Mainnet gesetzt, endlich, positiv und unter dem Höchstwert liegt.
pub(crate) fn valid_mainnet_cap(cap: Option<f64>) -> bool {
    cap.is_some_and(|c| c.is_finite() && c > 0.0 && c <= MAINNET_HARD_CAP)
}
