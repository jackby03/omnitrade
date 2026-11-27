//! Not-Aus-Flag für die GUI. Gleiches Dateiprotokoll wie `omnitrade-cli kill`:
//! Datei vorhanden = Not-Aus gesetzt (`OMNITRADE_KILL_FILE` oder `<temp>/omnitrade.kill`).

use std::io;
use std::path::{Path, PathBuf};

/// Umgebungsvariable, die den Pfad der Flag-Datei überschreibt (wie im CLI).
pub const KILL_FILE_ENV: &str = "OMNITRADE_KILL_FILE";

/// Standardpfad der Flag-Datei.
#[must_use]
pub fn default_path() -> PathBuf {
    std::env::var_os(KILL_FILE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("omnitrade.kill"))
}

/// Setzt den Not-Aus. Idempotent.
///
/// # Errors
/// I/O-Fehler beim Schreiben der Datei.
pub fn engage(path: &Path) -> io::Result<()> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::fs::write(path, format!("kill {secs}\n"))
}

/// `true`, wenn der Not-Aus gesetzt ist.
#[must_use]
pub fn is_engaged(path: &Path) -> bool {
    path.exists()
}

/// Hebt den Not-Aus auf. Fehlende Datei ist kein Fehler.
///
/// # Errors
/// I/O-Fehler, wenn die Datei existiert, aber nicht gelöscht werden kann.
pub fn clear(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engage_and_clear_roundtrip() {
        // Arrange
        let path = std::env::temp_dir().join(format!("omnitrade-ui-kill-{}", std::process::id()));
        let _ = clear(&path);

        // Act & Assert
        assert!(!is_engaged(&path));
        engage(&path).expect("engage");
        assert!(is_engaged(&path));
        engage(&path).expect("idempotent");
        clear(&path).expect("clear");
        assert!(!is_engaged(&path));
        clear(&path).expect("clear without file is ok");
    }
}
