//! OKX private-API signing:
//! `OK-ACCESS-SIGN = base64(HMAC-SHA256(secret, timestamp + METHOD + request_path + body))`.

use crate::error::ExchangeError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// OKX signature generator.
pub struct OkxSigner;

impl OkxSigner {
    /// Computes the `OK-ACCESS-SIGN` value.
    ///
    /// `request_path` includes the query string for GET requests; `body` is the raw JSON
    /// body (empty for GET).
    ///
    /// # Errors
    /// `InvalidCredentials` if the secret cannot key the HMAC.
    pub fn sign(
        timestamp: &str,
        method: &str,
        request_path: &str,
        body: &str,
        secret: &str,
    ) -> Result<String, ExchangeError> {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|_| ExchangeError::InvalidCredentials("Invalid secret key".to_string()))?;
        mac.update(timestamp.as_bytes());
        mac.update(method.to_uppercase().as_bytes());
        mac.update(request_path.as_bytes());
        mac.update(body.as_bytes());
        Ok(STANDARD.encode(mac.finalize().into_bytes()))
    }
}

/// Formats milliseconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SS.mmmZ` (UTC),
/// the timestamp format OKX expects in `OK-ACCESS-TIMESTAMP`.
#[must_use]
pub fn iso_timestamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let millis = ms % 1000;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_independent_computation() {
        // Erwartungswert unabhängig mit Python (hmac/hashlib/base64) berechnet.
        let sig = OkxSigner::sign(
            "2020-12-08T09:08:57.715Z",
            "get",
            "/api/v5/account/balance?ccy=BTC",
            "",
            "22582BD0CFF14C41EDBF1AB98506286D",
        )
        .expect("ok");
        assert_eq!(sig, "HiZhvSfMtWJA3uUIVXV3a/bSXNPCWvYFXoGCVS8V4zY=");
    }

    #[test]
    fn body_and_method_change_signature() {
        let a = OkxSigner::sign("t", "POST", "/p", "{}", "s").expect("ok");
        let b = OkxSigner::sign("t", "POST", "/p", "{\"a\":1}", "s").expect("ok");
        let c = OkxSigner::sign("t", "GET", "/p", "{}", "s").expect("ok");
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn formats_iso_timestamps() {
        assert_eq!(iso_timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_timestamp(1_696_436_700_123), "2023-10-04T16:25:00.123Z");
        assert_eq!(iso_timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }
}
