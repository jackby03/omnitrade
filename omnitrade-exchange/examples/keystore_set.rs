//! Legt Testnet-Zugangsdaten im Keystore ab (Windows Credential Manager).
//! Liest OMNITRADE_API_KEY / OMNITRADE_API_SECRET aus der Umgebung, damit
//! nichts in Shell-Historie oder Argumentlisten landet. Aufruf über
//! `scripts/set-testnet-keys.ps1`.

use omnitrade_exchange::{ApiCredentials, Keystore, Network};

fn main() {
    let key = std::env::var("OMNITRADE_API_KEY").unwrap_or_default();
    let secret = std::env::var("OMNITRADE_API_SECRET").unwrap_or_default();

    // Nur Testnet: Mainnet-Keys haben ein eigenes Gate und gehören nicht hierher.
    let creds = ApiCredentials::new(key, secret, Network::Testnet);
    match Keystore::save_credentials("binance", &creds) {
        Ok(()) => println!("OK: Testnet-Zugang unter omnitrade/binance/testnet gespeichert."),
        Err(e) => {
            eprintln!("FEHLER: {e}");
            std::process::exit(1);
        }
    }
}
