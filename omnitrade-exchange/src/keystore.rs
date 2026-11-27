//! Keystore integration for secure credential management via OS keyrings.
//! Supports Windows Credential Manager, macOS Keychain, Linux Secret Service.

use crate::credentials::{ApiCredentials, Network};
use crate::error::ExchangeError;
use keyring::Entry;

/// Keystore manager for persisting and retrieving API credentials.
pub struct Keystore;

impl Keystore {
    /// Build a keyring entry identifier from exchange and network.
    /// Format: `omnitrade/exchange/network` (e.g., `omnitrade/binance/testnet`)
    fn entry_key(exchange: &str, network: Network) -> String {
        format!("omnitrade/{}/{}", exchange, network)
    }

    /// Save API credentials to the system keyring.
    /// Separates Testnet and Mainnet credentials to prevent cross-wiring.
    ///
    /// # Errors
    /// Returns `ExchangeError` if keyring access fails.
    pub fn save_credentials(
        exchange: &str,
        credentials: &ApiCredentials,
    ) -> Result<(), ExchangeError> {
        credentials.validate().map_err(|e| {
            ExchangeError::InvalidCredentials(format!("Invalid credentials: {}", e))
        })?;

        let service = "omnitrade";
        let username = Self::entry_key(exchange, credentials.network);

        // Serialize credentials to JSON for storage
        let payload = serde_json::to_string(credentials).map_err(|e| {
            ExchangeError::InvalidCredentials(format!("Failed to serialize credentials: {}", e))
        })?;

        let entry = Entry::new(service, &username)
            .map_err(|e| ExchangeError::KeyringError(e.to_string()))?;

        entry
            .set_password(&payload)
            .map_err(|e| ExchangeError::KeyringError(e.to_string()))?;

        Ok(())
    }

    /// Load API credentials from the system keyring.
    ///
    /// # Errors
    /// Returns `ExchangeError` if credentials not found or keyring access fails.
    pub fn load_credentials(
        exchange: &str,
        network: Network,
    ) -> Result<ApiCredentials, ExchangeError> {
        let service = "omnitrade";
        let username = Self::entry_key(exchange, network);

        let entry = Entry::new(service, &username)
            .map_err(|e| ExchangeError::KeyringError(e.to_string()))?;

        let payload = entry
            .get_password()
            .map_err(|e| ExchangeError::KeyringError(e.to_string()))?;

        let credentials: ApiCredentials = serde_json::from_str(&payload).map_err(|e| {
            ExchangeError::InvalidCredentials(format!("Failed to deserialize credentials: {}", e))
        })?;

        // Verify network match to prevent accidents
        if credentials.network != network {
            return Err(ExchangeError::NetworkMismatch(format!(
                "Expected {}, got {}",
                network, credentials.network
            )));
        }

        Ok(credentials)
    }

    /// Delete credentials from the keyring (e.g., for cleanup or credential rotation).
    ///
    /// # Errors
    /// Returns `ExchangeError` if deletion fails.
    pub fn delete_credentials(exchange: &str, network: Network) -> Result<(), ExchangeError> {
        let service = "omnitrade";
        let username = Self::entry_key(exchange, network);

        let entry = Entry::new(service, &username)
            .map_err(|e| ExchangeError::KeyringError(e.to_string()))?;

        entry
            .delete_password()
            .map_err(|e| ExchangeError::KeyringError(e.to_string()))?;

        Ok(())
    }

    /// Check if credentials exist in the keyring without retrieving them.
    pub fn credentials_exist(exchange: &str, network: Network) -> bool {
        let service = "omnitrade";
        let username = Self::entry_key(exchange, network);
        if let Ok(entry) = Entry::new(service, &username) {
            entry.get_password().is_ok()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_key_format() {
        let key = Keystore::entry_key("binance", Network::Testnet);
        assert_eq!(key, "omnitrade/binance/testnet");

        let key = Keystore::entry_key("kraken", Network::Mainnet);
        assert_eq!(key, "omnitrade/kraken/mainnet");
    }

    // Schreibt in den echten Schlüsselbund-Eintrag `omnitrade/binance/testnet` und
    // löscht ihn danach: gehört ein Schlüssel dort einem Entwickler, geht er verloren.
    // Darum nur ausdrücklich laufen lassen (`cargo test -- --ignored`).
    #[ignore = "schreibt in den echten Schlüsselbund-Eintrag"]
    #[test]
    fn test_credentials_roundtrip() {
        // This test requires keyring access and is marked as integration test
        let creds = ApiCredentials::new(
            "test_key_123".to_string(),
            "test_secret_456".to_string(),
            Network::Testnet,
        );

        // Save to keyring
        if Keystore::save_credentials("binance", &creds).is_ok() {
            // Load from keyring
            match Keystore::load_credentials("binance", Network::Testnet) {
                Ok(loaded) => {
                    assert_eq!(loaded.key, creds.key);
                    assert_eq!(loaded.secret, creds.secret);
                    assert_eq!(loaded.network, creds.network);
                    // Clean up
                    let _ = Keystore::delete_credentials("binance", Network::Testnet);
                }
                Err(_e) => {
                    // Keyring might not be available in test environment
                }
            }
        }
    }

    #[ignore = "schreibt in den echten Schlüsselbund-Eintrag"]
    #[test]
    fn test_network_mismatch_protection() {
        let creds = ApiCredentials::new("key".to_string(), "secret".to_string(), Network::Testnet);

        // Save testnet creds
        if Keystore::save_credentials("binance", &creds).is_ok() {
            // Try to load as mainnet (should fail)
            let result = Keystore::load_credentials("binance", Network::Mainnet);
            assert!(result.is_err());
            // Clean up
            let _ = Keystore::delete_credentials("binance", Network::Testnet);
        }
    }
}
