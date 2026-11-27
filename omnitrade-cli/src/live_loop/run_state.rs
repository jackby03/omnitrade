//! Laufbestand über Neustarts. Der Lauf merkt sich in einer Datei neben der Statusdatei,
//! wie viel Basiswährung er selbst hält. Nach einem Neustart zählt genau dieser Teil als
//! Laufposition; der übrige Bestand im Konto bleibt Vorbestand und wird nie verkauft.
//!
//! Ohne Statusdatei gibt es keinen Ort zum Merken: dann bleibt der Lauf wie bisher und
//! behandelt den Bestand beim Start als Vorbestand.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::warn;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Saved {
    symbol: String,
    network: String,
    owned_base: f64,
}

/// Merkt den Laufbestand einer Spot-Position über Prozessneustarts hinweg.
#[derive(Debug)]
pub(crate) struct RunState {
    path: Option<PathBuf>,
    symbol: String,
    network: String,
    resume: Option<f64>,
    last: Option<f64>,
    /// Vorbestand beim ersten Abruf dieses Laufs (nur Spot).
    start: Option<f64>,
}

impl RunState {
    /// Öffnet den Laufbestand neben `status` (`live_300.json` -> `live_300.run.json`).
    pub(crate) fn open(status: Option<&Path>, symbol: &str, network: &str) -> Self {
        let path = status.map(|p| p.with_extension("run.json"));
        let resume = path.as_deref().and_then(|p| load(p, symbol, network));
        Self {
            path,
            symbol: symbol.to_string(),
            network: network.to_string(),
            resume,
            last: None,
            start: None,
        }
    }

    /// Vorbestand dieses Laufs, festgehalten beim ersten Abruf. Mit `adopt` gehört der
    /// ganze Bestand dem Lauf, sonst nur der gemerkte Laufbestand.
    pub(crate) fn baseline(&mut self, held: f64, adopt: bool) -> f64 {
        let resume = self.resume;
        *self.start.get_or_insert_with(|| {
            if adopt {
                0.0
            } else {
                baseline_for(held, resume)
            }
        })
    }

    /// Laufbestand aus dem letzten Lauf, falls vorhanden und zu diesem Symbol gehörig.
    pub(crate) fn resume(&self) -> Option<f64> {
        self.resume
    }

    /// Schreibt den Laufbestand, wenn er sich geändert hat. Ein Schreibfehler wird geloggt,
    /// der Lauf läuft weiter; der Wert wird dann beim nächsten Versuch erneut geschrieben.
    pub(crate) fn record(&mut self, owned: f64) {
        let Some(path) = &self.path else {
            return;
        };
        if self.last == Some(owned) {
            return;
        }
        let saved = Saved {
            symbol: self.symbol.clone(),
            network: self.network.clone(),
            owned_base: owned,
        };
        let written = serde_json::to_string_pretty(&saved)
            .map_err(io::Error::other)
            .and_then(|text| fs::write(path, text));
        match written {
            Ok(()) => self.last = Some(owned),
            Err(e) => warn!("Laufbestand nicht schreibbar ({}): {e}", path.display()),
        }
    }
}

fn load(path: &Path, symbol: &str, network: &str) -> Option<f64> {
    let text = fs::read_to_string(path).ok()?;
    let saved: Saved = match serde_json::from_str(&text) {
        Ok(saved) => saved,
        Err(e) => {
            warn!("Laufbestand {} unlesbar, ignoriert: {e}", path.display());
            return None;
        }
    };
    if saved.symbol != symbol || saved.network != network {
        warn!(
            "Laufbestand gehört zu {}/{}, nicht zu {symbol}/{network}: ignoriert",
            saved.symbol, saved.network
        );
        return None;
    }
    (saved.owned_base.is_finite() && saved.owned_base >= 0.0).then_some(saved.owned_base)
}

/// Vorbestand beim ersten Abruf eines Laufs. Ohne gemerkten Laufbestand ist der gesamte
/// Bestand Vorbestand. Mit gemerktem Laufbestand bleibt alles darüber Vorbestand; der
/// gemerkte Wert wird auf den tatsächlichen Bestand begrenzt, falls extern verkauft wurde.
pub(crate) fn baseline_for(held: f64, owned: Option<f64>) -> f64 {
    match owned {
        None => held,
        Some(owned) => held - owned.clamp(0.0, held.max(0.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_status(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omni-runstate-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        dir.join("live_test.json")
    }

    #[test]
    fn recorded_owned_base_is_resumed_for_same_symbol_and_network() {
        let status = temp_status("roundtrip");
        let mut first = RunState::open(Some(&status), "BTCUSDC", "testnet");
        assert_eq!(first.resume(), None);
        first.record(0.0042);

        let second = RunState::open(Some(&status), "BTCUSDC", "testnet");
        assert_eq!(second.resume(), Some(0.0042));
        fs::remove_dir_all(status.parent().expect("dir")).ok();
    }

    #[test]
    fn state_of_other_symbol_or_network_is_ignored() {
        let status = temp_status("mismatch");
        RunState::open(Some(&status), "BTCUSDC", "testnet").record(0.5);

        assert_eq!(
            RunState::open(Some(&status), "ETHUSDC", "testnet").resume(),
            None
        );
        assert_eq!(
            RunState::open(Some(&status), "BTCUSDC", "mainnet").resume(),
            None
        );
        fs::remove_dir_all(status.parent().expect("dir")).ok();
    }

    #[test]
    fn unreadable_or_negative_state_is_ignored() {
        let status = temp_status("garbage");
        let path = status.with_extension("run.json");
        fs::write(&path, "kein json").expect("write");
        assert_eq!(
            RunState::open(Some(&status), "BTCUSDC", "testnet").resume(),
            None
        );

        fs::write(
            &path,
            r#"{"symbol":"BTCUSDC","network":"testnet","owned_base":-1.0}"#,
        )
        .expect("write");
        assert_eq!(
            RunState::open(Some(&status), "BTCUSDC", "testnet").resume(),
            None
        );
        fs::remove_dir_all(status.parent().expect("dir")).ok();
    }

    #[test]
    fn without_status_file_nothing_is_persisted() {
        let mut state = RunState::open(None, "BTCUSDC", "testnet");
        state.record(1.0);
        assert_eq!(state.resume(), None);
    }

    #[test]
    fn baseline_keeps_foreign_holdings_out_of_the_run() {
        // Ohne gemerkten Lauf gehört der ganze Bestand dem Konto (Vorbestand).
        assert_eq!(baseline_for(1.33, None), 1.33);
        // Mit gemerktem Laufbestand 0,002 gehört nur dieser Teil dem Lauf.
        assert!((baseline_for(1.33, Some(0.002)) - 1.328).abs() < 1e-12);
        // Extern verkauft: der Lauf kann nicht mehr besitzen, als vorhanden ist.
        assert_eq!(baseline_for(0.001, Some(0.002)), 0.0);
    }
}
