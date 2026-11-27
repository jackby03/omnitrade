//! Kraken private-API signing: `API-Sign = base64(HMAC-SHA512(base64decode(secret),
//! uri_path + SHA256(nonce + post_data)))`.

use crate::error::ExchangeError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};

/// Kraken signature generator.
pub struct KrakenSigner;

impl KrakenSigner {
    /// Computes the `API-Sign` header value.
    ///
    /// `post_data` is the url-encoded body, which must contain `nonce=<nonce>`.
    ///
    /// # Errors
    /// `InvalidCredentials` if the secret is not valid base64.
    pub fn sign(
        uri_path: &str,
        nonce: u64,
        post_data: &str,
        secret_b64: &str,
    ) -> Result<String, ExchangeError> {
        let secret = STANDARD
            .decode(secret_b64.trim())
            .map_err(|_| ExchangeError::InvalidCredentials("Secret ist kein Base64".to_string()))?;
        let mut sha = Sha256::new();
        sha.update(nonce.to_string().as_bytes());
        sha.update(post_data.as_bytes());
        let mut mac = Hmac::<Sha512>::new_from_slice(&secret)
            .map_err(|_| ExchangeError::InvalidCredentials("Invalid secret key".to_string()))?;
        mac.update(uri_path.as_bytes());
        mac.update(&sha.finalize());
        Ok(STANDARD.encode(mac.finalize().into_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Testvektor aus der Kraken-Dokumentation (unabhängig mit Python nachgerechnet).
    const SECRET: &str =
        "kQH5HW/8p1uGOVjbgWA7FunAmGO8lsSUXNsu3eow76sz84Q18fWxnyRzBHCd3pd5nE9qa99HAZtuZuj6F1huXg==";
    const POST: &str =
        "nonce=1616492376594&ordertype=limit&pair=XBTUSD&price=37500&type=buy&volume=1.25";

    #[test]
    fn matches_documented_test_vector() {
        // Act
        let sig = KrakenSigner::sign("/0/private/AddOrder", 1616492376594, POST, SECRET)
            .expect("valid secret");

        // Assert
        assert_eq!(
            sig,
            "4/dpxb3iT4tp/ZCVEwSnEsLxx0bqyhLpdfOpc6fn7OR8+UClSV5n9E6aSS8MPtnRfp32bAb0nmbRn6H8ndwLUQ=="
        );
    }

    #[test]
    fn different_path_changes_signature() {
        let a = KrakenSigner::sign("/0/private/AddOrder", 1616492376594, POST, SECRET).expect("ok");
        let b = KrakenSigner::sign("/0/private/Balance", 1616492376594, POST, SECRET).expect("ok");
        assert_ne!(a, b);
    }

    #[test]
    fn rejects_non_base64_secret() {
        let err = KrakenSigner::sign("/0/private/Balance", 1, "nonce=1", "!!!kein base64!!!");
        assert!(matches!(err, Err(ExchangeError::InvalidCredentials(_))));
    }
}
