//! Risiko- und Eigenkapital-Helfer der Live-Schleife: Symbolzerlegung, Bewertung,
//! Liquidationsabstand und Positionsdeckel.

use omnitrade_core::Side;
use omnitrade_exchange::{Account, FillDetails, PositionSnapshot};

use super::run_state::baseline_for;
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

/// Realisierter PnL beim Verringern einer Perpetual-Position. Er entspricht dem
/// zuletzt bekannten unrealisierten PnL des geschlossenen Anteils. Ein Vorzeichenwechsel
/// schliesst die alte Position ganz. Naeherung: die Abweichung zwischen Marktpreis und
/// Fuellpreis fehlt. Gebuehren kommen getrennt dazu, siehe [`perp_gebuehr`].
pub(crate) fn realisiert_beim_schliessen(
    vorher: &PositionSnapshot,
    nachher: &PositionSnapshot,
) -> f64 {
    if !vorher.is_perpetual || vorher.base_qty == 0.0 {
        return 0.0;
    }
    let alt = vorher.base_qty;
    let neu = nachher.base_qty;
    let geschlossen = if neu == 0.0 || neu.signum() != alt.signum() {
        alt.abs()
    } else {
        (alt.abs() - neu.abs()).max(0.0)
    };
    if geschlossen <= 0.0 {
        return 0.0;
    }
    vorher.unrealized_pnl * (geschlossen / alt.abs()).min(1.0)
}

/// Gebuehr einer Ausfuehrung in der Abrechnungswaehrung, Vorzeichen wie von der Boerse
/// (Kosten negativ). Abrechnungs- und Quote-Waehrung zaehlen 1:1 (dieselbe Annahme wie
/// in [`bewertungs_waehrung`]), eine Gebuehr in der Basis wird zum Fuellpreis
/// umgerechnet. `None` bei fremder Waehrung oder unbrauchbarer Zahl.
pub(crate) fn gebuehr_in_abrechnung(
    fill: &FillDetails,
    base: &str,
    quote: &str,
    settle: Option<&str>,
) -> Option<f64> {
    if !fill.fee.is_finite() {
        return None;
    }
    if fill.fee == 0.0 {
        return Some(0.0);
    }
    let gleich = |ccy: &str| ccy.eq_ignore_ascii_case(&fill.fee_ccy);
    if gleich(quote) || settle.is_some_and(gleich) {
        return Some(fill.fee);
    }
    if gleich(base) && fill.avg_price.is_finite() && fill.avg_price > 0.0 {
        return Some(fill.fee * fill.avg_price);
    }
    None
}

/// Beitrag einer Perpetual-Ausfuehrung zum Laufeigenkapital: die Gebuehr in der
/// Abrechnungswaehrung. Fehlen die Ausfuehrungsdaten oder ist die Waehrung fremd, wird
/// nichts gebucht und gewarnt: das Laufeigenkapital ist dann um diese Gebuehr zu hoch.
pub(crate) fn perp_gebuehr(fill: Option<&FillDetails>, symbol: &str, settle: Option<&str>) -> f64 {
    let Some(fill) = fill else {
        tracing::warn!("Gebuehr unbekannt (keine Ausfuehrungsdaten): Laufeigenkapital ohne sie");
        return 0.0;
    };
    let wert = split_symbol(symbol)
        .and_then(|(base, quote)| gebuehr_in_abrechnung(fill, &base, &quote, settle));
    wert.unwrap_or_else(|| {
        tracing::warn!(
            "Gebuehr {} {} nicht bewertbar: Laufeigenkapital ohne sie",
            fill.fee,
            fill.fee_ccy
        );
        0.0
    })
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
    resume: Option<f64>,
}

impl LongOnlyGuard {
    pub(crate) fn new() -> Self {
        Self {
            start_base: None,
            resume: None,
        }
    }

    /// Übernimmt eine bestehende Position als Laufposition (wie [`ExposureLimit::adopt_existing`]).
    pub(crate) fn adopt_existing(mut self, adopt: bool) -> Self {
        if adopt {
            self.start_base = Some(0.0);
        }
        self
    }

    /// Gemerkter Laufbestand aus dem letzten Lauf (siehe `run_state`). Ohne Adoption gilt
    /// nur dieser Teil als Laufbestand, der Rest bleibt Vorbestand.
    #[must_use]
    pub(crate) fn resume_owned(mut self, owned: Option<f64>) -> Self {
        self.resume = owned;
        self
    }

    /// `held` ist die aktuelle NETTO-Position (`held_base`).
    pub(crate) fn check(&mut self, side: Side, quantity: f64, held: f64) -> Result<(), String> {
        let resume = self.resume;
        let start = *self
            .start_base
            .get_or_insert_with(|| baseline_for(held, resume));
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
    resume: Option<f64>,
}

impl ExposureLimit {
    pub(crate) fn new(cap: f64) -> Self {
        Self {
            cap,
            start_base: None,
            allow_shorts: false,
            resume: None,
        }
    }

    /// Gemerkter Laufbestand aus dem letzten Lauf (siehe `run_state`). Wirkt nur ohne Adoption.
    #[must_use]
    pub(crate) fn resume_owned(mut self, owned: Option<f64>) -> Self {
        self.resume = owned;
        self
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
        let resume = self.resume;
        let start = *self
            .start_base
            .get_or_insert_with(|| baseline_for(held, resume));
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

/// Deckel und Long-only-Bremse eines Laufs. Beide starten mit dem gemerkten Laufbestand
/// (`resume`), außer der Lauf übernimmt den Bestand ausdrücklich (`adopt`).
pub(crate) fn run_guards(
    cap: Option<f64>,
    allow_shorts: bool,
    adopt: bool,
    resume: Option<f64>,
) -> (Option<ExposureLimit>, LongOnlyGuard) {
    let exposure = cap.map(|cap| {
        ExposureLimit::new(cap)
            .allow_shorts(allow_shorts)
            .adopt_existing(adopt)
            .resume_owned(resume)
    });
    let long_only = LongOnlyGuard::new()
        .adopt_existing(adopt)
        .resume_owned(resume);
    (exposure, long_only)
}

/// `true` für Derivate: Perpetuals (`-SWAP`, `XPERP`, `_UM_`) und datierte Futures
/// (`...-YYMMDD`). Dort zählt die gesamte Position gegen den Deckel, ein Vorbestand gibt es nicht.
pub(crate) fn is_derivative_symbol(symbol: &str) -> bool {
    let s = symbol.to_ascii_uppercase();
    let datiert = s
        .rsplit('-')
        .next()
        .is_some_and(|seg| seg.len() == 6 && seg.chars().all(|c| c.is_ascii_digit()));
    s.contains("XPERP") || s.contains("-SWAP") || s.contains("_UM_") || datiert
}

/// `true`, wenn der Deckel für Mainnet gesetzt, endlich, positiv und unter dem Höchstwert liegt.
pub(crate) fn valid_mainnet_cap(cap: Option<f64>) -> bool {
    cap.is_some_and(|c| c.is_finite() && c > 0.0 && c <= MAINNET_HARD_CAP)
}

#[cfg(test)]
mod derivative_tests {
    use super::is_derivative_symbol;

    #[test]
    fn perpetuals_and_dated_futures_are_derivatives_spot_is_not() {
        assert!(is_derivative_symbol("HYPE-USD_UM_XPERP-310801"));
        assert!(is_derivative_symbol("BTC-USD-SWAP"));
        assert!(is_derivative_symbol("BTC-USD-261030"));
        assert!(!is_derivative_symbol("BTC-USDC"));
        assert!(!is_derivative_symbol("BTCUSDT"));
    }
}
