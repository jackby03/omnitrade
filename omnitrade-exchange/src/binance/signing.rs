//! HMAC-SHA256 signing for Binance API requests.
//! Per Binance documentation: https://binance-docs.github.io/apidocs/

use crate::error::ExchangeError;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Binance HMAC-SHA256 signature generator.
pub struct BinanceSigner;

impl BinanceSigner {
    /// Sign a query string using HMAC-SHA256.
    /// Returns hex-encoded signature ready for API requests.
    ///
    /// # Arguments
    /// * `query_string` - URL-encoded query parameters (without signature)
    /// * `secret` - API secret key
    ///
    /// # Errors
    /// Currently infallible, but errors may be added for invalid UTF-8 handling.
    pub fn sign(query_string: &str, secret: &str) -> Result<String, ExchangeError> {
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|_| ExchangeError::InvalidCredentials("Invalid secret key".to_string()))?;

        mac.update(query_string.as_bytes());
        let result = mac.finalize();
        let code_bytes = result.into_bytes();

        Ok(hex::encode(code_bytes))
    }

    /// Combine query parameters and signature into a complete query string.
    pub fn build_signed_query(query_string: &str, signature: &str) -> String {
        if query_string.is_empty() {
            format!("signature={}", signature)
        } else {
            format!("{}&signature={}", query_string, signature)
        }
    }

    /// Sign a complete request with timestamp and recvWindow.
    /// This is the typical flow for Binance API calls.
    pub fn sign_request(
        query_string: &str,
        secret: &str,
        timestamp: i64,
    ) -> Result<String, ExchangeError> {
        let timestamped = if query_string.is_empty() {
            format!("timestamp={}", timestamp)
        } else {
            format!("{}&timestamp={}", query_string, timestamp)
        };
        let signature = Self::sign(&timestamped, secret)?;
        Ok(Self::build_signed_query(&timestamped, &signature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_empty_query() {
        // Example from Binance docs (approximate)
        let secret =
            "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e9DZZkDd2sSKLCnQI46b4z9HBoVheWYNmgQ9AlT94PJt3p1EQ";
        let result = BinanceSigner::sign("", secret);
        assert!(result.is_ok());
        // Signature should be a non-empty hex string
        let sig = result.unwrap();
        assert!(!sig.is_empty());
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_sign_with_params() {
        let secret = "test_secret_key";
        let query = "symbol=BTCUSDT&side=BUY&quantity=1&type=MARKET";
        let result = BinanceSigner::sign(query, secret);
        assert!(result.is_ok());
        let sig = result.unwrap();
        // Signature should be deterministic for same inputs
        let sig2 = BinanceSigner::sign(query, secret).unwrap();
        assert_eq!(sig, sig2);
    }

    #[test]
    fn test_sign_deterministic() {
        let secret = "fixed_secret";
        let query = "test=data";
        let sig1 = BinanceSigner::sign(query, secret).expect("sign failed");
        let sig2 = BinanceSigner::sign(query, secret).expect("sign failed");
        assert_eq!(sig1, sig2, "Same inputs must produce same signature");
    }

    #[test]
    fn test_build_signed_query_empty() {
        let sig = "abc123def456";
        let result = BinanceSigner::build_signed_query("", sig);
        assert_eq!(result, "signature=abc123def456");
    }

    #[test]
    fn test_build_signed_query_with_params() {
        let query = "symbol=BTCUSDT";
        let sig = "abc123def456";
        let result = BinanceSigner::build_signed_query(query, sig);
        assert_eq!(result, "symbol=BTCUSDT&signature=abc123def456");
    }

    #[test]
    fn test_sign_request_flow() {
        let secret = "secret123";
        let query = "symbol=ETHUSDT";
        let timestamp = 1694527382000i64;
        let result = BinanceSigner::sign_request(query, secret, timestamp);
        assert!(result.is_ok());
        let signed = result.unwrap();
        assert!(signed.contains("symbol=ETHUSDT"));
        assert!(signed.contains(&timestamp.to_string()));
        assert!(signed.contains("signature="));
    }

    #[test]
    fn test_sign_request_empty_query_has_no_leading_ampersand() {
        let signed =
            BinanceSigner::sign_request("", "secret123", 1694527382000i64).expect("sign failed");
        assert!(signed.starts_with("timestamp=1694527382000&signature="));
    }

    #[test]
    fn test_signature_differs_with_different_secret() {
        let query = "symbol=BTCUSDT&side=BUY";
        let sig1 = BinanceSigner::sign(query, "secret1").unwrap();
        let sig2 = BinanceSigner::sign(query, "secret2").unwrap();
        assert_ne!(
            sig1, sig2,
            "Different secrets must produce different signatures"
        );
    }

    #[test]
    fn test_signature_differs_with_different_query() {
        let secret = "same_secret";
        let sig1 = BinanceSigner::sign("query1", secret).unwrap();
        let sig2 = BinanceSigner::sign("query2", secret).unwrap();
        assert_ne!(
            sig1, sig2,
            "Different queries must produce different signatures"
        );
    }
}
