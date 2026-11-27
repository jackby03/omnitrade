//! Kill-Switch-Befehl: Not-Aus für einen laufenden Live-Handel.

use crate::kill_switch;
use std::path::Path;

/// Setzt oder löscht den Not-Aus.
///
/// Gesetzt stoppt er die laufende Live-Schleife (keine neuen Orders, offene
/// Orders werden storniert) und verhindert neue Live-Läufe. Bestände werden
/// nicht automatisch verkauft. Mit `reset` wird der Not-Aus wieder aufgehoben.
///
/// # Errors
/// Gibt einen Fehler zurück, wenn die Flag-Datei nicht geschrieben oder gelöscht werden kann.
pub fn handle_kill(reset: bool, flag: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if reset {
        kill_switch::clear(flag)?;
        eprintln!("Not-Aus aufgehoben ({})", flag.display());
    } else {
        kill_switch::engage(flag)?;
        eprintln!("Not-Aus gesetzt ({})", flag.display());
        eprintln!("Laufender Live-Handel stoppt und storniert offene Orders.");
        eprintln!("Bestände werden nicht verkauft. Aufheben mit: omnitrade-cli kill --reset");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_sets_and_reset_clears_flag() {
        let flag = std::env::temp_dir().join(format!("omnitrade-kill-cmd-{}", std::process::id()));
        handle_kill(false, &flag).expect("kill");
        assert!(kill_switch::is_engaged(&flag));
        handle_kill(true, &flag).expect("reset");
        assert!(!kill_switch::is_engaged(&flag));
    }
}
