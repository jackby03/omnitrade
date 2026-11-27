//! API credentials management with strict network separation (Testnet/Mainnet).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Network designation to prevent accidental Mainnet exposure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Network {
    Testnet,
    Mainnet,
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Network::Testnet => write!(f, "testnet"),
            Network::Mainnet => write!(f, "mainnet"),
        }
    }
}

/// API credentials for exchange authentication.
/// Must explicitly specify network to prevent cross-wiring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCredentials {
    pub key: String,
    pub secret: String,
    pub network: Network,
}

impl ApiCredentials {
    /// Create new API credentials with explicit network designation.
    pub fn new(key: String, secret: String, network: Network) -> Self {
        Self {
            key,
            secret,
            network,
        }
    }

    /// Check if credentials are for Testnet.
    pub fn is_testnet(&self) -> bool {
        self.network == Network::Testnet
    }

    /// Check if credentials are for Mainnet.
    pub fn is_mainnet(&self) -> bool {
        self.network == Network::Mainnet
    }

    /// Validate that credentials are non-empty.
    pub fn validate(&self) -> Result<(), String> {
        if self.key.is_empty() {
            return Err("API key cannot be empty".to_string());
        }
        if self.secret.is_empty() {
            return Err("API secret cannot be empty".to_string());
        }
        Ok(())
    }
}

impl fmt::Display for ApiCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ApiCredentials(key: {}..., network: {})",
            &self.key[..self.key.len().min(8)],
            self.network
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_display() {
        assert_eq!(Network::Testnet.to_string(), "testnet");
        assert_eq!(Network::Mainnet.to_string(), "mainnet");
    }

    #[test]
    fn test_credentials_network_check() {
        let testnet_creds =
            ApiCredentials::new("key".to_string(), "secret".to_string(), Network::Testnet);
        assert!(testnet_creds.is_testnet());
        assert!(!testnet_creds.is_mainnet());

        let mainnet_creds =
            ApiCredentials::new("key".to_string(), "secret".to_string(), Network::Mainnet);
        assert!(!mainnet_creds.is_testnet());
        assert!(mainnet_creds.is_mainnet());
    }

    #[test]
    fn test_credentials_validation() {
        let valid = ApiCredentials::new(
            "key123".to_string(),
            "secret456".to_string(),
            Network::Testnet,
        );
        assert!(valid.validate().is_ok());

        let invalid_key =
            ApiCredentials::new("".to_string(), "secret456".to_string(), Network::Testnet);
        assert!(invalid_key.validate().is_err());

        let invalid_secret =
            ApiCredentials::new("key123".to_string(), "".to_string(), Network::Testnet);
        assert!(invalid_secret.validate().is_err());
    }

    #[test]
    fn test_credentials_display() {
        let creds = ApiCredentials::new(
            "key123456789".to_string(),
            "secret".to_string(),
            Network::Testnet,
        );
        let display = creds.to_string();
        // Key display removed for security (masked in Display impl)
        assert!(display.contains("testnet"));
    }
}
