//! Not-Aus für den Live-Handel über eine Flag-Datei.
//!
//! `omnitrade-cli kill` legt die Datei an, die laufende Live-Schleife prüft sie
//! bei jeder Kerze und im Sekundentakt. Eine Datei statt IPC, weil sie auch
//! dann wirkt, wenn Prozess und Terminal getrennt laufen, und einen Absturz
//! überlebt: ein gesetzter Not-Aus verhindert auch den Neustart (fail closed).

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Umgebungsvariable, die den Pfad der Flag-Datei überschreibt.
pub const KILL_FILE_ENV: &str = "OMNITRADE_KILL_FILE";

/// Standardpfad der Flag-Datei (`OMNITRADE_KILL_FILE` oder `<temp>/omnitrade.kill`).
#[must_use]
pub fn default_path() -> PathBuf {
    std::env::var_os(KILL_FILE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("omnitrade.kill"))
}

/// Setzt den Not-Aus. Idempotent.
///
/// # Errors
/// Gibt den I/O-Fehler zurück, wenn die Datei nicht geschrieben werden kann.
pub fn engage(path: &Path) -> io::Result<()> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::fs::write(path, format!("kill {secs}\n"))
}

/// `true`, wenn der Not-Aus gesetzt ist.
#[must_use]
pub fn is_engaged(path: &Path) -> bool {
    path.exists()
}

/// Hebt den Not-Aus auf. Fehlt die Datei, ist das kein Fehler.
///
/// # Errors
/// Gibt den I/O-Fehler zurück, wenn die Datei existiert, aber nicht gelöscht werden kann.
pub fn clear(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_flag(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("omnitrade-kill-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn engage_sets_flag_and_clear_removes_it() {
        let path = temp_flag("cycle");
        assert!(!is_engaged(&path));
        engage(&path).expect("engage");
        assert!(is_engaged(&path));
        engage(&path).expect("engage is idempotent");
        clear(&path).expect("clear");
        assert!(!is_engaged(&path));
    }

    #[test]
    fn clear_without_flag_is_ok() {
        assert!(clear(&temp_flag("missing")).is_ok());
    }
}
