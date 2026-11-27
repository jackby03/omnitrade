//! Öffentliche OKX-EEA-Endpunkte gegen die echte API. Braucht Netzwerk, daher ignoriert.
//!
//! Start: `cargo test -p omnitrade-exchange --test okx_eea_public -- --ignored`
//!
//! Keine Zugangsdaten nötig: Die Instrumentenliste ist öffentlich. Die Dummy-Keys
//! werden nur benötigt, weil jeder REST-Aufruf im Client signiert wird.

use omnitrade_exchange::okx::{InstType, OkxRestClient};
use omnitrade_exchange::{ApiCredentials, Network};

const EEA_REST: &str = "https://eea.okx.com";

fn dummy_client() -> OkxRestClient {
    let creds = ApiCredentials::new("dummy-key".into(), "dummy-secret".into(), Network::Mainnet);
    OkxRestClient::new(creds, "dummy-passphrase".into())
        .expect("client builds")
        .with_base_url(EEA_REST)
}

#[tokio::test]
#[ignore = "braucht Netzwerkzugang zu eea.okx.com"]
async fn eea_btc_usdt_swap_rules_match_expected_contract_size() {
    // Arrange
    let client = dummy_client();
    // Act
    let inst = client
        .load_instrument("BTC-USDT-SWAP", InstType::Swap)
        .await
        .expect("EEA instrument loads");
    // Assert: 1 Kontrakt = 0.01 BTC, Schrittweite und Minimum 0.01 Kontrakte
    assert_eq!(inst.inst_type, InstType::Swap);
    assert!((inst.ct_val - 0.01).abs() < 1e-12);
    assert!((inst.lot_sz - 0.01).abs() < 1e-12);
    assert!((inst.min_sz - 0.01).abs() < 1e-12);
}

#[tokio::test]
#[ignore = "braucht Netzwerkzugang zu eea.okx.com"]
async fn unknown_swap_instrument_is_rejected() {
    // Arrange
    let client = dummy_client();
    // Act
    let result = client
        .load_instrument("NOPE-USDT-SWAP", InstType::Swap)
        .await;
    // Assert
    assert!(result.is_err(), "unbekanntes Instrument darf nicht laden");
}
