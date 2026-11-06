use super::*;

fn client(network: Network) -> Result<OkxRestClient, ExchangeError> {
    OkxRestClient::new(
        ApiCredentials::new("key".into(), "secret".into(), network),
        "pass".into(),
    )
}

#[test]
fn constructor_validates_inputs_and_demo_flag() {
    assert!(client(Network::Testnet).expect("demo").is_demo());
    assert!(!client(Network::Mainnet).expect("prod").is_demo());
    let no_pass = OkxRestClient::new(
        ApiCredentials::new("k".into(), "s".into(), Network::Testnet),
        String::new(),
    );
    assert!(matches!(no_pass, Err(ExchangeError::InvalidCredentials(_))));
    let no_key = OkxRestClient::new(
        ApiCredentials::new(String::new(), "s".into(), Network::Testnet),
        "p".into(),
    );
    assert!(matches!(no_key, Err(ExchangeError::InvalidCredentials(_))));
}

#[test]
fn base_url_can_be_switched_and_is_normalized() {
    let c = client(Network::Mainnet)
        .expect("client")
        .with_base_url("https://eea.okx.com/");
    assert_eq!(c.base_url, "https://eea.okx.com");
}

#[test]
fn envelope_ok_and_errors() {
    // Act & Assert
    let data =
        parse_envelope(r#"{"code":"0","msg":"","data":[{"ordId":"1","sCode":"0"}]}"#).expect("ok");
    assert_eq!(data.len(), 1);

    match parse_envelope(r#"{"code":"50111","msg":"Invalid OK-ACCESS-KEY","data":[]}"#) {
        Err(ExchangeError::ApiError(m)) => assert!(m.starts_with("50111")),
        other => panic!("expected ApiError, got {other:?}"),
    }
    match parse_envelope(
        r#"{"code":"1","msg":"All operations failed","data":[{"sCode":"51008","sMsg":"insufficient balance"}]}"#,
    ) {
        Err(ExchangeError::ApiError(m)) => assert!(m.contains("insufficient balance")),
        other => panic!("expected ApiError, got {other:?}"),
    }
    match parse_envelope(r#"{"code":"0","msg":"","data":[{"sCode":"51000","sMsg":"bad"}]}"#) {
        Err(ExchangeError::ApiError(m)) => assert!(m.starts_with("51000")),
        other => panic!("expected ApiError, got {other:?}"),
    }
    assert!(parse_envelope("kein json").is_err());
}

#[test]
fn maps_pending_order() {
    // Arrange
    let v: Value = serde_json::from_str(
        r#"{"instId":"BTC-USDT","ordId":"312269865356374016","side":"buy","ordType":"limit","px":"50000.5","sz":"0.01","accFillSz":"0.004","state":"partially_filled","cTime":"1700000000000","uTime":"1700000001000"}"#,
    )
    .expect("json");

    // Act
    let o = parse_order(&v).expect("order");

    // Assert
    assert_eq!(o.id, 312_269_865_356_374_016);
    assert_eq!((o.side, o.order_type), (Side::Buy, OrderType::Limit));
    assert_eq!(
        (o.price, o.quantity, o.filled_quantity),
        (50000.5, 0.01, 0.004)
    );
    assert_eq!(o.status, OrderStatus::PartiallyFilled);
    assert_eq!(
        (o.created_at_ms, o.updated_at_ms),
        (1_700_000_000_000, 1_700_000_001_000)
    );
}

#[test]
fn maps_states_and_rejects_bad_ids() {
    let state = |s: &str| {
        let v: Value = serde_json::from_str(&format!(
            r#"{{"instId":"BTC-USDT","ordId":"7","side":"sell","ordType":"market","px":"","sz":"1","accFillSz":"1","state":"{s}"}}"#
        ))
        .expect("json");
        parse_order(&v).expect("order").status
    };
    assert_eq!(state("filled"), OrderStatus::Filled);
    assert_eq!(state("canceled"), OrderStatus::Cancelled);
    assert_eq!(state("live"), OrderStatus::New);
    let bad: Value =
        serde_json::from_str(r#"{"instId":"X","ordId":"abc","side":"buy"}"#).expect("json");
    assert!(parse_order(&bad).is_none());
}

#[test]
fn maps_balances() {
    // Arrange
    let data: Vec<Value> = serde_json::from_str(
        r#"[{"details":[{"ccy":"USDT","availBal":"100.5","frozenBal":"4.5"},{"ccy":"BTC","availBal":"","frozenBal":"0"}]}]"#,
    )
    .expect("json");

    // Act
    let account = parse_balance(&data);

    // Assert
    assert_eq!(account.balances["USDT"].free, 100.5);
    assert_eq!(account.balances["USDT"].total(), 105.0);
    assert_eq!(account.balances["BTC"].total(), 0.0);
}

#[tokio::test]
async fn place_order_validates_before_any_request() {
    // Arrange
    let c = client(Network::Testnet).expect("client");

    // Act & Assert
    let zero = OrderRequest::market("BTC-USDT".into(), Side::Buy, 0.0);
    assert!(matches!(
        c.place_order(&zero).await,
        Err(ExchangeError::InvalidRequest(_))
    ));
    let mut no_price = OrderRequest::limit("BTC-USDT".into(), Side::Buy, 1.0, 1.0);
    no_price.price = None;
    assert!(matches!(
        c.place_order(&no_price).await,
        Err(ExchangeError::InvalidRequest(_))
    ));
    let mut stop = OrderRequest::market("BTC-USDT".into(), Side::Buy, 1.0);
    stop.order_type = OrderType::StopMarket;
    assert!(matches!(
        c.place_order(&stop).await,
        Err(ExchangeError::InvalidRequest(_))
    ));
}
