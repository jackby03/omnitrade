//! OKX-Instrumentdaten (`/api/v5/public/instruments`) und Umrechnung zwischen
//! Basiswährung und Kontrakten.
//!
//! Bei SWAP (Perpetual) ist `sz` eine Anzahl Kontrakte; ein Kontrakt entspricht
//! `ctVal` Basiswährung. Die Live-Schleife rechnet ausschließlich in Basiswährung,
//! die Umrechnung passiert deshalb hier, am Rand des Adapters.

use serde_json::Value;

use crate::error::ExchangeError;

/// Toleranz gegen Gleitkomma-Rauschen, z. B. `0.03 / 0.01 = 2.9999999999999996`.
const ROUNDING_EPS: f64 = 1e-9;

/// Instrumenttyp laut OKX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstType {
    Spot,
    Swap,
    /// XPERP-Perpetual (instType=FUTURES), z. B. `BTC-USD_UM_XPERP-310404`.
    /// Mechanisch identisch zu SWAP (Kontrakte, ctVal/lotSz/minSz), nur andere
    /// instType-Kennung und ID-Format. EEA-Konten handeln ausschliesslich XPERP.
    Futures,
}

impl InstType {
    /// Wert des Query-Parameters `instType` der OKX-API.
    #[must_use]
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Spot => "SPOT",
            Self::Swap => "SWAP",
            Self::Futures => "FUTURES",
        }
    }

    /// `true` für kontraktbasierte Instrumente (SWAP und XPERP/FUTURES), deren
    /// Menge in Kontrakten angegeben wird. SPOT rechnet in Basiswährung.
    #[must_use]
    pub fn is_contract_based(self) -> bool {
        matches!(self, Self::Swap | Self::Futures)
    }
}

/// Datierter Terminkontrakt an der ID: das letzte Segment nach `-` ist ein
/// Datum `YYMMDD`. Beispiele: `BTC-USD_UM-261030`, `BTC-USD-261030`.
///
/// Auf dem EEA-Demokonto ist dies der EINZIGE handelbare Kontrakttyp: SWAP
/// antwortet mit `50124` (kein Handelsrecht) und XPERP-Instrumente fehlen dort
/// ganz (`51001`). Gemessen am 21.10.2026.
#[must_use]
pub fn is_dated_future(inst_id: &str) -> bool {
    match inst_id.rsplit_once('-') {
        Some((_, tail)) => tail.len() == 6 && tail.chars().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

/// Kontraktbasiert (Menge in Kontrakten): SWAP, XPERP und datierte Futures.
#[must_use]
pub fn is_contract_id(inst_id: &str) -> bool {
    let up = inst_id.to_uppercase();
    up.ends_with("-SWAP") || up.contains("_XPERP-") || is_dated_future(&up)
}

/// InstType aus der OKX-Notation eines kontraktbasierten Symbols.
///
/// `None` heißt: kein kontraktbasiertes Symbol (Spot oder unbekannt) - der
/// Aufrufer entscheidet dann, statt dass hier geraten wird.
#[must_use]
pub fn inst_type_for_symbol(symbol: &str) -> Option<InstType> {
    let up = symbol.to_uppercase();
    if up.ends_with("-SWAP") {
        Some(InstType::Swap)
    } else if up.contains("_XPERP-") || is_dated_future(&up) {
        Some(InstType::Futures)
    } else {
        None
    }
}

/// Handelsregeln eines OKX-Instruments.
#[derive(Debug, Clone, PartialEq)]
pub struct Instrument {
    pub inst_id: String,
    pub inst_type: InstType,
    /// Basiswährung je Kontrakt (SWAP). Bei SPOT 1.0.
    pub ct_val: f64,
    /// Schrittweite der Menge in Kontrakten (SWAP) bzw. Basiswährung (SPOT).
    pub lot_sz: f64,
    /// Mindestmenge in Kontrakten (SWAP) bzw. Basiswährung (SPOT).
    pub min_sz: f64,
    /// Währung, in der der Kontrakt abgerechnet wird (`settleCcy`).
    ///
    /// Bei XPERP ist das USDC, obwohl das Symbol `USD` sagt — wer die Equity aus
    /// dem Kontostand rechnet, muss diese Währung nehmen, sonst findet er keinen
    /// Bestand und bewertet das Konto mit 0.
    pub settle_ccy: Option<String>,
}

impl Instrument {
    /// Liest ein Instrument-Objekt aus der Antwort von `/public/instruments`.
    ///
    /// # Errors
    /// `ApiError`, wenn Pflichtfelder fehlen, nicht lesbar sind oder der Typ unbekannt ist.
    pub fn parse(v: &Value) -> Result<Self, ExchangeError> {
        let inst_id = text_field(v, "instId")?;
        let inst_type = match text_field(v, "instType")?.as_str() {
            "SPOT" => InstType::Spot,
            "SWAP" => InstType::Swap,
            "FUTURES" => InstType::Futures,
            other => {
                return Err(ExchangeError::ApiError(format!(
                    "{inst_id}: unbekannter instType '{other}'"
                )))
            }
        };
        let ct_val = match inst_type {
            InstType::Swap | InstType::Futures => number_field(v, "ctVal")?,
            InstType::Spot => 1.0,
        };
        // `settleCcy` ist die Abrechnungswaehrung des Kontrakts (XPERP: USDC, obwohl
        // das Symbol `USD` traegt). Fehlt sie, bleibt es bei `None` — der Aufrufer
        // rechnet dann wie bisher mit der Quote des Symbols.
        let settle_ccy = v
            .get("settleCcy")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let instrument = Self {
            inst_id,
            inst_type,
            ct_val,
            lot_sz: number_field(v, "lotSz")?,
            min_sz: number_field(v, "minSz")?,
            settle_ccy,
        };
        instrument.validate()?;
        Ok(instrument)
    }

    fn validate(&self) -> Result<(), ExchangeError> {
        let positive = [self.ct_val, self.lot_sz, self.min_sz]
            .iter()
            .all(|x| x.is_finite() && *x > 0.0);
        if positive {
            Ok(())
        } else {
            Err(ExchangeError::ApiError(format!(
                "{}: ctVal, lotSz und minSz müssen positiv sein",
                self.inst_id
            )))
        }
    }

    /// Basiswährung → Kontrakte, abgerundet auf `lot_sz`. Nur für SWAP.
    ///
    /// Abrunden ist die sichere Richtung: die Position wird nie größer als angefordert.
    ///
    /// # Errors
    /// `InvalidRequest` bei SPOT-Instrumenten, bei nicht-positiver Menge oder wenn
    /// das Ergebnis unter `min_sz` liegt (fail closed statt stillem Auffüllen).
    pub fn base_to_contracts(&self, base_qty: f64) -> Result<f64, ExchangeError> {
        if matches!(self.inst_type, InstType::Spot) {
            return Err(ExchangeError::InvalidRequest(format!(
                "{}: Kontraktumrechnung nur für SWAP/XPERP",
                self.inst_id
            )));
        }
        if !base_qty.is_finite() || base_qty <= 0.0 {
            return Err(ExchangeError::InvalidRequest(
                "Quantity must be positive".to_string(),
            ));
        }
        let steps = (base_qty / self.ct_val / self.lot_sz + ROUNDING_EPS).floor();
        let contracts = steps * self.lot_sz;
        if contracts + ROUNDING_EPS < self.min_sz {
            return Err(ExchangeError::InvalidRequest(format!(
                "{}: Menge {base_qty} ergibt {contracts} Kontrakte, Minimum ist {}",
                self.inst_id, self.min_sz
            )));
        }
        Ok(contracts)
    }

    /// Kontrakte → Basiswährung. Für SWAP und XPERP (ctVal ist bei beiden in der Base).
    #[must_use]
    pub fn contracts_to_base(&self, contracts: f64) -> f64 {
        contracts * self.ct_val
    }
}

fn text_field(v: &Value, key: &str) -> Result<String, ExchangeError> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| ExchangeError::ApiError(format!("Instrument ohne Feld '{key}'")))
}

fn number_field(v: &Value, key: &str) -> Result<f64, ExchangeError> {
    text_field(v, key)?
        .parse::<f64>()
        .map_err(|_| ExchangeError::ApiError(format!("Feld '{key}' ist keine Zahl")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Echte Antwort von `eea.okx.com/api/v5/public/instruments?instType=SWAP&instId=BTC-USDT-SWAP`.
    fn btc_swap() -> Instrument {
        let json = serde_json::json!({
            "instId": "BTC-USDT-SWAP", "instType": "SWAP", "ctVal": "0.01",
            "ctValCcy": "BTC", "lotSz": "0.01", "minSz": "0.01", "ctType": "linear",
        });
        Instrument::parse(&json).expect("fixture parses")
    }

    #[test]
    fn parses_swap_fields_from_okx_payload() {
        // Arrange + Act
        let inst = btc_swap();
        // Assert
        assert_eq!(inst.inst_type, InstType::Swap);
        assert_eq!(inst.ct_val, 0.01);
        assert_eq!(inst.lot_sz, 0.01);
        assert_eq!(inst.min_sz, 0.01);
    }

    #[test]
    fn liest_die_abrechnungswaehrung_aus_settle_ccy() {
        // XPERP des EEA-Kontos: Symbol sagt USD, abgerechnet wird in USDC.
        let xperp = serde_json::json!({
            "instId": "SOL-USD_UM_XPERP-310404", "instType": "FUTURES", "ctVal": "0.01",
            "ctValCcy": "SOL", "settleCcy": "USDC", "lotSz": "1", "minSz": "1",
        });
        let inst = Instrument::parse(&xperp).expect("XPERP parses");
        assert_eq!(inst.settle_ccy.as_deref(), Some("USDC"));

        // Fehlt das Feld, bleibt es leer - der Aufrufer faellt auf die Quote zurueck.
        let ohne = serde_json::json!({
            "instId": "BTC-USDT", "instType": "SPOT", "lotSz": "0.00000001", "minSz": "0.00001",
        });
        let spot = Instrument::parse(&ohne).expect("SPOT parses");
        assert_eq!(spot.settle_ccy, None);
    }

    #[test]
    fn rounds_down_to_whole_contracts() {
        // Arrange
        let inst = btc_swap();
        // Act + Assert: 0.0149 BTC = 1.49 Kontrakte
        let c = inst.base_to_contracts(0.0149).expect("valid size");
        assert!((c - 1.49).abs() < 1e-9);
    }

    #[test]
    fn float_noise_does_not_lose_a_contract() {
        // Arrange: 0.03 / 0.01 ergibt in IEEE-754 2.9999999999999996
        let inst = btc_swap();
        // Act
        let c = inst.base_to_contracts(0.03).expect("valid size");
        // Assert
        assert!((c - 3.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_size_below_minimum_instead_of_rounding_up() {
        // Arrange: 0.00005 BTC = 0.005 Kontrakte, Minimum 0.01
        let inst = btc_swap();
        // Act
        let result = inst.base_to_contracts(0.00005);
        // Assert
        assert!(matches!(result, Err(ExchangeError::InvalidRequest(_))));
    }

    #[test]
    fn rejects_non_positive_and_non_finite_quantities() {
        let inst = btc_swap();
        assert!(inst.base_to_contracts(0.0).is_err());
        assert!(inst.base_to_contracts(-1.0).is_err());
        assert!(inst.base_to_contracts(f64::NAN).is_err());
    }

    #[test]
    fn contracts_convert_back_to_base() {
        let inst = btc_swap();
        assert!((inst.contracts_to_base(5.0) - 0.05).abs() < 1e-12);
    }

    /// Echte Antwort von `eea.okx.com/api/v5/public/instruments?instType=FUTURES&instId=BTC-USD_UM_XPERP-310404`.
    fn btc_xperp() -> Instrument {
        let json = serde_json::json!({
            "instId": "BTC-USD_UM_XPERP-310404", "instType": "FUTURES", "ctVal": "0.0001",
            "ctValCcy": "BTC", "lotSz": "1", "minSz": "1", "ctType": "linear",
        });
        Instrument::parse(&json).expect("fixture parses")
    }

    #[test]
    fn parses_xperp_futures_fields_and_converts_contracts() {
        // Arrange
        let inst = btc_xperp();
        // Assert: instType FUTURES, ctVal 0.0001 BTC je Kontrakt
        assert_eq!(inst.inst_type, InstType::Futures);
        assert!(inst.inst_type.is_contract_based());
        assert_eq!(inst.ct_val, 0.0001);
        // Act: 0.0003 BTC = 3 Kontrakte
        let c = inst.base_to_contracts(0.0003).expect("valid size");
        // Assert
        assert!((c - 3.0).abs() < 1e-9);
        assert!((inst.contracts_to_base(3.0) - 0.0003).abs() < 1e-12);
    }

    #[test]
    fn spot_instrument_refuses_contract_conversion() {
        // Arrange
        let spot = Instrument::parse(&serde_json::json!({
            "instId": "BTC-USDT", "instType": "SPOT", "lotSz": "0.00000001", "minSz": "0.00001",
        }))
        .expect("spot fixture parses");
        // Act + Assert
        assert!(spot.base_to_contracts(0.1).is_err());
    }

    #[test]
    fn rejects_payload_with_missing_or_zero_fields() {
        let missing = serde_json::json!({"instId": "X-SWAP", "instType": "SWAP"});
        assert!(Instrument::parse(&missing).is_err());
        let zero = serde_json::json!({
            "instId": "X-SWAP", "instType": "SWAP", "ctVal": "0", "lotSz": "1", "minSz": "1",
        });
        assert!(Instrument::parse(&zero).is_err());
    }

    /// Datierter Terminkontrakt: das letzte Segment ist ein Datum YYMMDD.
    #[test]
    fn erkennt_datierte_futures() {
        assert!(is_dated_future("BTC-USD_UM-261030"));
        assert!(is_dated_future("btc-usd_um-261030"));
        assert!(is_dated_future("BTC-USD-261030"));
        // Gegenprobe: diese sind KEINE datierten Futures
        assert!(!is_dated_future("BTC-USDT-SWAP"));
        // XPERP traegt ebenfalls ein Datum im letzten Segment - dort greift
        // ohnehin die _XPERP-Regel, das Datum ist also kein Widerspruch.
        assert!(is_dated_future("BTC-USD_UM_XPERP-310404"));
        assert!(!is_dated_future("BTC-USDT"));
        assert!(!is_dated_future("BTC-USD-26103")); // 5 Stellen
        assert!(!is_dated_future("BTC-USD-2610300")); // 7 Stellen
        assert!(!is_dated_future("BTC-USD-ABCDEF"));
    }

    #[test]
    fn inst_type_aus_der_notierung() {
        assert_eq!(inst_type_for_symbol("BTC-USDT-SWAP"), Some(InstType::Swap));
        assert_eq!(
            inst_type_for_symbol("BTC-USD_UM_XPERP-310404"),
            Some(InstType::Futures)
        );
        assert_eq!(
            inst_type_for_symbol("BTC-USD_UM-261030"),
            Some(InstType::Futures)
        );
        assert_eq!(
            inst_type_for_symbol("BTC-USD-261030"),
            Some(InstType::Futures)
        );
        // Spot bleibt unentschieden - der Aufrufer entscheidet, hier wird nicht geraten
        assert_eq!(inst_type_for_symbol("BTC-USDT"), None);
    }

    #[test]
    fn contract_id_umfasst_alle_drei_familien() {
        assert!(is_contract_id("BTC-USDT-SWAP"));
        assert!(is_contract_id("BTC-USD_UM_XPERP-310404"));
        assert!(is_contract_id("BTC-USD_UM-261030"));
        assert!(!is_contract_id("BTC-USDT"));
        assert!(!is_contract_id("BTC-USDC"));
    }
}
