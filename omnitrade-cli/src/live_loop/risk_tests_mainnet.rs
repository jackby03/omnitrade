//! Tests der Risiko-Helfer: Eigenkapital, Liquidationsabstand, Positionsdeckel, Mainnet-Cap.

use super::risk::*;
use super::test_support::*;
use super::*;

#[test]
fn mainnet_cap_validation() {
    assert!(valid_mainnet_cap(Some(20.0)));
    assert!(valid_mainnet_cap(Some(MAINNET_HARD_CAP)));
    for bad in [
        None,
        Some(0.0),
        Some(-1.0),
        Some(MAINNET_HARD_CAP + 0.01),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        assert!(!valid_mainnet_cap(bad), "{bad:?}");
    }
}

#[tokio::test]
async fn mainnet_with_cap_above_hard_limit_is_rejected_before_any_exchange_call() {
    // Arrange
    let h = Harness::new("mainnet-over", BUY_BELOW_40K).with_cap(MAINNET_HARD_CAP + 1.0);

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Mainnet, 0.001)
        .await;

    // Assert
    assert!(matches!(result, Err(LiveLoopError::NetworkError(_))));
    assert!(h.log.lock().expect("log").is_empty());
}

#[tokio::test]
async fn mainnet_with_valid_cap_runs_and_blocks_orders_over_the_cap() {
    // Arrange: Deckel 20, Order 0.001 BTC @ 35000 = 35 > 20 → blockiert, kein Aufruf an die Börse.
    let h = Harness::new("mainnet-cap", BUY_BELOW_40K).with_cap(20.0);

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Mainnet, 0.001)
        .await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert!(
        h.placed.lock().expect("placed").is_empty(),
        "Order über Deckel darf nicht raus"
    );
}

#[tokio::test]
async fn testnet_cap_allows_orders_within_the_cap() {
    // Arrange: Deckel 50, Order 0.001 @ 35000 = 35 ≤ 50.
    let h = Harness::new("testnet-cap", BUY_BELOW_40K).with_cap(50.0);

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Testnet, 0.001)
        .await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(h.placed.lock().expect("placed").len(), 1);
}

#[tokio::test]
async fn non_positive_cap_is_rejected_even_on_testnet() {
    let h = Harness::new("cap-zero", BUY_BELOW_40K).with_cap(0.0);
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Testnet, 0.001)
        .await;
    assert!(matches!(result, Err(LiveLoopError::NetworkError(_))));
}
