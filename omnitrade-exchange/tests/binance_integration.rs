//! Integration test for Binance Testnet REST API.
//!
//! This test verifies that the REST client can:
//! - Connect to Binance Testnet
//! - Place orders
//! - Cancel orders
//! - Retrieve open orders
//! - Fetch account balances
//!
//! NOTE: These tests require valid Testnet API credentials stored in the system keyring.
//! Run with: cargo test -- --ignored
//!
//! To set up test credentials:
//! ```ignore
//! use omnitrade_exchange::{ApiCredentials, Network, Keystore};
//! let creds = ApiCredentials::new("TESTNET_KEY".to_string(), "TESTNET_SECRET".to_string(), Network::Testnet);
//! Keystore::save_credentials("binance", &creds).expect("Failed to save");
//! ```

use omnitrade_core::{OrderType, Side};
use omnitrade_exchange::{
    ApiCredentials, BinanceRestClient, ExchangeTrading, Keystore, Network, OrderRequest,
};

/// Helper to load or create test credentials
fn get_test_credentials() -> ApiCredentials {
    // Attempt to load from keyring
    if Keystore::credentials_exist("binance", Network::Testnet) {
        if let Ok(creds) = Keystore::load_credentials("binance", Network::Testnet) {
            return creds;
        }
    }

    // Fallback: Umgebungsvariablen (CI/CD). Ohne Zugänge brechen die Tests mit
    // klarer Meldung ab, statt mit Platzhaltern gegen die Börse zu laufen.
    let key = std::env::var("BINANCE_TESTNET_KEY");
    let secret = std::env::var("BINANCE_TESTNET_SECRET");
    match (key, secret) {
        (Ok(key), Ok(secret)) => ApiCredentials::new(key, secret, Network::Testnet),
        _ => panic!(
            "Keine Testnet-Zugänge: weder Keystore-Eintrag omnitrade/binance/testnet              noch BINANCE_TESTNET_KEY/BINANCE_TESTNET_SECRET"
        ),
    }
}

#[tokio::test]
#[ignore] // Only run when credentials are available
async fn test_binance_client_creation() {
    let creds = get_test_credentials();
    let result = BinanceRestClient::new(creds);

    // Should succeed with valid credentials (or placeholder)
    assert!(result.is_ok(), "Client creation failed");
}

#[tokio::test]
#[ignore] // Only run with valid Testnet credentials
async fn test_binance_invalid_credentials() {
    let invalid_creds = ApiCredentials::new("".to_string(), "secret".to_string(), Network::Testnet);
    let result = BinanceRestClient::new(invalid_creds);

    // Should fail with empty key
    assert!(
        result.is_err(),
        "Client creation should fail with empty key"
    );
}

#[tokio::test]
#[ignore] // Only run with valid Testnet credentials
async fn test_binance_testnet_creation() {
    // Verify that REST client can be created with Testnet credentials
    let creds = ApiCredentials::new(
        "test_key".to_string(),
        "test_secret".to_string(),
        Network::Testnet,
    );
    let result = BinanceRestClient::new(creds);

    // Should succeed with valid (even placeholder) credentials
    assert!(
        result.is_ok(),
        "Client creation should succeed with valid credentials"
    );
}

#[tokio::test]
#[ignore] // Only run with valid credentials - actually places order on Testnet
async fn test_place_market_order() {
    let creds = get_test_credentials();
    let client = match BinanceRestClient::new(creds) {
        Ok(c) => c,
        Err(e) => panic!("Zugaenge ungueltig: {:?}", e),
    };

    let req = OrderRequest::market("BTCUSDT".to_string(), Side::Buy, 0.001);

    match client.place_order(&req).await {
        Ok(order) => {
            println!("Order placed: {:?}", order.id);
            assert_eq!(order.symbol, "BTCUSDT");
            assert_eq!(order.side, Side::Buy);
            assert!(order.quantity > 0.0);
        }
        Err(e) => panic!("Testnet-Aufruf fehlgeschlagen: {:?}", e),
    }
}

#[tokio::test]
#[ignore] // Only run with valid credentials
async fn test_get_balance() {
    let creds = get_test_credentials();
    let client = match BinanceRestClient::new(creds) {
        Ok(c) => c,
        Err(e) => panic!("Zugaenge ungueltig: {:?}", e),
    };

    match client.get_balance().await {
        Ok(account) => {
            println!(
                "Account balances retrieved: {} assets",
                account.balances.len()
            );
            // Testnet typically has USDT and BNB balances
            assert!(!account.balances.is_empty(), "Account should have balances");
        }
        Err(e) => panic!("Testnet-Aufruf fehlgeschlagen: {:?}", e),
    }
}

#[tokio::test]
#[ignore] // Only run with valid credentials
async fn test_get_open_orders() {
    let creds = get_test_credentials();
    let client = match BinanceRestClient::new(creds) {
        Ok(c) => c,
        Err(e) => panic!("Zugaenge ungueltig: {:?}", e),
    };

    match client.get_open_orders().await {
        Ok(orders) => {
            // A successful call is the assertion: the list may be empty (no
            // open orders) or not. A len() comparison would be tautological -
            // len() is usize and can never be negative.
            println!("Open orders retrieved: {}", orders.len());
        }
        Err(e) => panic!("Testnet-Aufruf fehlgeschlagen: {:?}", e),
    }
}

#[test]
fn test_order_request_creation() {
    let market_req = OrderRequest::market("ETHUSDT".to_string(), Side::Sell, 0.5);
    assert_eq!(market_req.symbol, "ETHUSDT");
    assert_eq!(market_req.side, Side::Sell);
    assert_eq!(market_req.order_type, OrderType::Market);
    assert_eq!(market_req.price, None);

    let limit_req = OrderRequest::limit("BTCUSDT".to_string(), Side::Buy, 1.0, 45000.0);
    assert_eq!(limit_req.symbol, "BTCUSDT");
    assert_eq!(limit_req.side, Side::Buy);
    assert_eq!(limit_req.order_type, OrderType::Limit);
    assert_eq!(limit_req.price, Some(45000.0));
}
