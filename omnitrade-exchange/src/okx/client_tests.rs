use super::*;

#[test]
fn perpetual_symbols_are_accepted_and_malformed_ones_rejected() {
    // Arrange + Act + Assert
    assert_eq!(
        validate_symbol("btc-usdt-swap").expect("swap"),
        "BTC-USDT-SWAP"
    );
    assert_eq!(validate_symbol("BTC-USDT").expect("spot"), "BTC-USDT");
    assert!(validate_symbol("BTC-USDT-SWAP-X").is_err());
    assert!(validate_symbol("-USDT-SWAP").is_err());
}

/// Eine Kerzen-Nachricht, wie OKX sie auf `candle1m` schickt (bestaetigt).
fn kerzen_nachricht(ts: u64) -> String {
    format!(
        r#"{{"arg":{{"channel":"candle1m","instId":"BTC-USDT"}},
           "data":[["{ts}","100","101","99","100.5","1","1","1","1"]]}}"#
    )
}

#[tokio::test]
async fn baut_nach_abgerissener_verbindung_neu_auf() {
    // Arrange: ein lokaler Server, der je Verbindung genau eine Kerze schickt
    // und den Socket dann hart fallen laesst (so wie OKX es am 21.10.2026 tat).
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let adresse = listener.local_addr().expect("addr");
    let verbindungen = Arc::new(AtomicUsize::new(0));
    let zaehler = Arc::clone(&verbindungen);
    tokio::spawn(async move {
        loop {
            let Ok((strom, _)) = listener.accept().await else {
                return;
            };
            let nummer = zaehler.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(strom).await else {
                    return;
                };
                let _ = ws.next().await; // Abo-Nachricht des Clients
                let _ = ws
                    .send(Message::Text(kerzen_nachricht(1_000 * nummer).into()))
                    .await;
                // Kurz warten, damit die Kerze wirklich beim Client ankommt -
                // ein sofortiger Abbruch verwirft sie auf Loopback.
                tokio::time::sleep(Duration::from_millis(200)).await;
                drop(ws); // kein Close-Frame: harter Abbruch
            });
        }
    });

    let mut client = OkxClient::with_urls(format!("ws://{adresse}"), format!("ws://{adresse}"));
    let mut kerzen = client
        .subscribe_candles("BTC-USDT", "1m")
        .await
        .expect("abo");

    // Act
    client.connect().await.expect("verbinden");
    let erste = tokio::time::timeout(Duration::from_secs(5), kerzen.recv())
        .await
        .expect("erste Kerze kam nicht")
        .expect("Kanal zu");
    let zweite = tokio::time::timeout(Duration::from_secs(15), kerzen.recv())
        .await
        .expect("nach dem Abbruch kam keine zweite Kerze - kein Reconnect")
        .expect("Kanal zu");

    // Assert: der Client hat neu verbunden und liefert weiter.
    assert!(zweite.timestamp_ms > erste.timestamp_ms);
    assert!(
        verbindungen.load(Ordering::SeqCst) >= 2,
        "der Client hat die Verbindung nicht neu aufgebaut"
    );
}

#[test]
fn backoff_beginnt_bei_einer_sekunde_und_deckelt_bei_dreissig() {
    // Arrange + Act
    let stufen: Vec<u64> = (1..=8)
        .map(|v| wiederverbindung_wartezeit(v).as_secs())
        .collect();

    // Assert
    assert_eq!(stufen, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    assert_eq!(wiederverbindung_wartezeit(0).as_secs(), 1);
}

#[test]
fn region_selects_eea_hosts_only_when_requested() {
    // Arrange + Act
    let global = OkxClient::for_region(Network::Testnet, false);
    let eea = OkxClient::for_region(Network::Testnet, true);
    // Assert
    assert_eq!(global.public_url, DEMO_PUBLIC_URL);
    assert_eq!(eea.public_url, EEA_DEMO_PUBLIC_URL);
    assert_eq!(eea.business_url, EEA_DEMO_BUSINESS_URL);
}

#[tokio::test]
async fn candles_go_to_business_and_books_to_public() {
    // Arrange
    let mut client = OkxClient::new();

    // Act
    client
        .subscribe_candles("btc-usdt", "1h")
        .await
        .expect("candles");
    client.subscribe_depth("ETH-USDT").await.expect("depth");

    // Assert
    let candle: serde_json::Value =
        serde_json::from_str(&client.business_subscriptions()[0]).expect("json");
    assert_eq!(candle["op"], "subscribe");
    assert_eq!(candle["args"][0]["channel"], "candle1H");
    assert_eq!(candle["args"][0]["instId"], "BTC-USDT");
    let book: serde_json::Value =
        serde_json::from_str(&client.public_subscriptions()[0]).expect("json");
    assert_eq!(book["args"][0]["channel"], "books");
    assert_eq!(book["args"][0]["instId"], "ETH-USDT");
    assert_eq!(client.public_subscriptions().len(), 1);
    assert_eq!(client.business_subscriptions().len(), 1);
}

#[tokio::test]
async fn rejects_bad_symbol_and_interval() {
    // Arrange
    let mut client = OkxClient::new();

    // Act & Assert
    assert!(client.subscribe_candles("BTCUSDT", "1m").await.is_err());
    assert!(client.subscribe_depth("BTC/USDT").await.is_err());
    assert!(client.subscribe_candles("BTC-USDT", "7m").await.is_err());
    assert!(client.business_subscriptions().is_empty());
    assert!(client.public_subscriptions().is_empty());
}

#[test]
fn for_network_selects_matching_endpoints_and_info() {
    // Act
    let demo = OkxClient::for_network(Network::Testnet);
    let prod = OkxClient::for_network(Network::Mainnet);

    // Assert
    assert_eq!(demo.public_url, DEMO_PUBLIC_URL);
    assert_eq!(demo.business_url, DEMO_BUSINESS_URL);
    assert_eq!(prod.public_url, PUBLIC_URL);
    assert_eq!(prod.name(), "OKX");
    assert!(prod.supported_intervals().contains(&"1h"));
}

#[test]
fn validate_symbol_accepts_all_okx_contract_forms() {
    assert!(validate_symbol("BTC-USDT").is_ok());
    assert!(validate_symbol("BTC-USDT-SWAP").is_ok());
    assert!(validate_symbol("BTC-USD-261030").is_ok());
    assert!(validate_symbol("SOL-USD_UM_XPERP-310404").is_ok());
    assert!(validate_symbol("sol-usd_um_xperp-310404").is_ok());
    // Regression: XPERP-Symbole muss die Kerzen-Subscription annehmen
    // (Live-Lauf 21.10.2026 brach vorher an dieser Validierung ab).
    let norm = validate_symbol("sol-usd_um_xperp-310404").expect("XPERP ok");
    assert_eq!(norm, "SOL-USD_UM_XPERP-310404");
}

#[test]
fn validate_symbol_rejects_malformed_symbols() {
    assert!(validate_symbol("BTCUSDT").is_err());
    assert!(validate_symbol("BTC/USDT").is_err());
    assert!(validate_symbol("BTC-USDT-FOO").is_err());
    assert!(validate_symbol("BTC-USDT-26103").is_err()); // 5 Ziffern
    assert!(validate_symbol("BTC-USD-26AB30").is_err()); // nicht alle Ziffern
    assert!(validate_symbol("BTC-USD_UM_XPERP-3104").is_err());
    assert!(validate_symbol("-USDT").is_err());
    assert!(validate_symbol("BTC-").is_err());
}
