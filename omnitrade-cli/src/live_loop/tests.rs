//! Tests des Schleifenablaufs: Signale, Kill-Switch, Fehlerpfade, Mainnet-Sperre.

use super::test_support::*;
use super::*;

#[tokio::test]
async fn buy_signal_places_market_order_with_configured_quantity() {
    // Arrange
    let h = Harness::new("buy", BUY_BELOW_40K);

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Testnet, 0.002)
        .await;

    // Assert
    assert!(result.is_ok());
    let placed = h.placed.lock().expect("placed");
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].symbol, "BTCUSDT");
    assert!(matches!(placed[0].side, Side::Buy));
    assert!((placed[0].quantity - 0.002).abs() < f64::EPSILON);
}

#[tokio::test]
async fn subscribes_before_connect_and_disconnects_at_end() {
    // Arrange
    let h = Harness::new("order", BUY_BELOW_40K);

    // Act
    let result = h
        .run(&[45000.0], Some(1000.0), Network::Testnet, 0.001)
        .await;

    // Assert
    assert!(result.is_ok());
    assert_eq!(
        *h.log.lock().expect("log"),
        vec!["subscribe", "connect", "disconnect"]
    );
}

#[tokio::test]
async fn engaged_kill_switch_blocks_orders_cancels_open_orders_and_disconnects() {
    let h = Harness::new("kill", BUY_BELOW_40K);
    kill_switch::engage(&h.kill).expect("engage");

    h.run(&[30_000.0], Some(1000.0), Network::Testnet, 0.001)
        .await
        .expect("halt is a graceful end");

    assert!(h.placed.lock().expect("placed").is_empty());
    assert_eq!(*h.cancelled.lock().expect("cancelled"), vec!["77"]);
    assert_eq!(
        h.log.lock().expect("log").last().map(String::as_str),
        Some("disconnect")
    );
}

#[tokio::test]
async fn without_kill_switch_nothing_is_cancelled() {
    let h = Harness::new("nokill", BUY_BELOW_40K);

    h.run(&[30_000.0], Some(1000.0), Network::Testnet, 0.001)
        .await
        .expect("run");

    assert_eq!(h.placed.lock().expect("placed").len(), 1);
    assert!(h.cancelled.lock().expect("cancelled").is_empty());
}

#[tokio::test]
async fn hold_signal_places_no_order() {
    // Arrange
    let h = Harness::new("hold", BUY_BELOW_40K);

    // Act
    let result = h
        .run(&[45000.0], Some(1000.0), Network::Testnet, 0.001)
        .await;

    // Assert
    assert!(result.is_ok());
    assert!(h.placed.lock().expect("placed").is_empty());
}

#[tokio::test]
async fn sell_signal_opens_short_only_when_allow_shorts() {
    // Arrange: flaches Konto, Sell-Signal (Kurs über 40k).
    let h = Harness::new("short-allowed", SELL_ABOVE_40K).with_allow_shorts();

    // Act
    let result = h
        .run(&[45000.0], Some(1000.0), Network::Testnet, 0.001)
        .await;

    // Assert: Sell-Order wird platziert (Short-Eröffnung im net mode).
    assert!(result.is_ok());
    {
        let placed = h.placed.lock().expect("placed");
        assert_eq!(placed.len(), 1);
        assert!(matches!(placed[0].side, Side::Sell));
    }

    // Ohne das Flag bleibt der Lauf long-only: dieselbe Situation, keine Order.
    let h2 = Harness::new("short-default", SELL_ABOVE_40K);
    h2.run(&[45000.0], Some(1000.0), Network::Testnet, 0.001)
        .await
        .expect("run");
    let placed2 = h2.placed.lock().expect("placed");
    assert!(placed2.is_empty());
}

#[tokio::test]
async fn balance_failure_blocks_order_fail_closed() {
    // Arrange
    let h = Harness::new("nobalance", BUY_BELOW_40K);

    // Act
    let result = h.run(&[35000.0], None, Network::Testnet, 0.001).await;

    // Assert
    assert!(result.is_ok(), "Einzelfehler beenden die Schleife nicht");
    assert!(h.placed.lock().expect("placed").is_empty());
}

#[tokio::test]
async fn oversized_order_is_rejected_by_risk_guard_and_loop_continues() {
    // Arrange: 1.0 BTC * 35000 > 10000 USDT Testnet-Limit
    let h = Harness::new("oversize", BUY_BELOW_40K);

    // Act
    let result = h
        .run(&[35000.0, 35000.0], Some(1000.0), Network::Testnet, 1.0)
        .await;

    // Assert
    assert!(result.is_ok());
    assert!(h.placed.lock().expect("placed").is_empty());
    assert_eq!(
        h.log.lock().expect("log").last().map(String::as_str),
        Some("disconnect")
    );
}

#[tokio::test]
async fn mainnet_is_rejected_before_any_exchange_call() {
    // Arrange
    let h = Harness::new("mainnet", BUY_BELOW_40K);

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Mainnet, 0.001)
        .await;

    // Assert
    assert!(matches!(result, Err(LiveLoopError::NetworkError(_))));
    assert!(h.log.lock().expect("log").is_empty());
}

#[tokio::test]
async fn non_positive_quantity_is_rejected() {
    // Arrange
    let h = Harness::new("qty", BUY_BELOW_40K);

    // Act
    let zero = h.run(&[35000.0], Some(1000.0), Network::Testnet, 0.0).await;
    let negative = h
        .run(&[35000.0], Some(1000.0), Network::Testnet, -1.0)
        .await;

    // Assert
    assert!(matches!(zero, Err(LiveLoopError::ExchangeError(_))));
    assert!(matches!(negative, Err(LiveLoopError::ExchangeError(_))));
    assert!(h.log.lock().expect("log").is_empty());
}

#[tokio::test]
async fn missing_script_file_returns_script_file_error() {
    // Arrange
    let h = Harness::new("missing", BUY_BELOW_40K);
    fs::remove_file(&h.script).expect("remove script");

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Testnet, 0.001)
        .await;

    // Assert
    assert!(matches!(result, Err(LiveLoopError::ScriptFileError(_))));
}

#[test]
fn test_live_loop_error_display() {
    let err = LiveLoopError::ScriptFileError(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "file not found",
    ));
    assert!(err.to_string().contains("Script file error"));
}

fn kerze(ts: u64) -> Candle {
    Candle::new(ts, 100.0, 101.0, 99.0, 100.5, 1.0)
}

#[test]
fn intervall_sekunden_kennt_die_hausintervalle() {
    // Arrange + Act + Assert
    assert_eq!(intervall_sekunden("1m"), 60);
    assert_eq!(intervall_sekunden("15m"), 900);
    assert_eq!(intervall_sekunden("1h"), 3_600);
    assert_eq!(intervall_sekunden("4h"), 14_400);
    assert_eq!(intervall_sekunden("1d"), 86_400);
    // Unbekanntes Intervall darf nicht panisch werden, sondern 1 s ergeben.
    assert_eq!(intervall_sekunden(""), 1);
}

#[test]
fn kanal_gilt_nach_drei_intervallen_als_still() {
    // Arrange
    let start = 1_000_000u64;

    // Act + Assert
    // Signatur: kanal_still(letzte_kerze_ms, jetzt_ms, intervall_s)
    // 1m: Grenze max(3 x 60 s, 60 s) = 180 s
    assert!(!kanal_still(start, start + 120_000, 60));
    assert!(kanal_still(start, start + 200_000, 60));
    // 1h: Grenze 3 h - ein einzelner Aussetzer ist noch keine Funkstille.
    assert!(!kanal_still(start, start + 2 * 3_600_000, 3_600));
    assert!(kanal_still(start, start + 4 * 3_600_000, 3_600));
    // Uhr zurueckgesprungen (NTP): keine Funkstille, kein Unterlauf.
    assert!(!kanal_still(start + 5_000, start, 60));
    assert!(!kanal_still(start, start, 60));
}

#[test]
fn puffer_aus_rest_ergaenzt_und_entdoppelt() {
    // Arrange: der Puffer kennt 30, 40, 50 s; REST liefert 10..40
    let alt = vec![kerze(30_000), kerze(40_000), kerze(50_000)];
    let neu = vec![kerze(10_000), kerze(20_000), kerze(30_000), kerze(40_000)];

    // Act
    let puffer = puffer_aus_rest(&alt, neu);

    // Assert: aufsteigend, jede Kerze genau einmal, die juengere aus dem Puffer bleibt.
    let ts: Vec<u64> = puffer.iter().map(|c| c.timestamp_ms).collect();
    assert_eq!(ts, vec![10_000, 20_000, 30_000, 40_000, 50_000]);
}

#[test]
fn puffer_aus_rest_ohne_rest_kerzen_behaelt_den_bestand() {
    // Arrange
    let alt = vec![kerze(30_000), kerze(40_000)];

    // Act
    let puffer = puffer_aus_rest(&alt, Vec::new());

    // Assert
    assert_eq!(puffer.len(), 2);
    assert_eq!(puffer[1].timestamp_ms, 40_000);
}

#[test]
fn puffer_aus_rest_deckelt_auf_die_puffergroesse() {
    // Arrange
    let alt: Vec<Candle> = Vec::new();
    let neu: Vec<Candle> = (0..(MAX_CANDLE_BUFFER as u64 + 40))
        .map(|i| kerze(1_000 + i * 1_000))
        .collect();

    // Act
    let puffer = puffer_aus_rest(&alt, neu);

    // Assert: gedeckelt, und zwar von OBEN - die neueste Kerze bleibt.
    assert_eq!(puffer.len(), MAX_CANDLE_BUFFER);
    assert_eq!(
        puffer.last().expect("letzte").timestamp_ms,
        1_000 + 539 * 1_000
    );
}

/// Das Buch folgt der BOERSE, nicht dem eigenen Zaehler: eine Position, die
/// dieser Lauf nie geordert hat (Handschluss, Stop, ein zweiter Prozess), steht
/// nach der naechsten Kerze im Status. Genau das fehlte am 21.10.2026, als der
/// Lauf -0.1 SOL fuehrte, waehrend die Boerse flach war.
#[tokio::test]
async fn buch_uebernimmt_die_boersenposition_ohne_eigenen_auftrag() {
    // Arrange: Strategie handelt nie, die Boerse meldet -0.4.
    let h = Harness::new("buch-boerse", "if close() < 1 { buy() }")
        .with_position(-0.4)
        .with_status_file("buch-boerse");

    // Act
    let result = h.run(&[100.0], Some(1000.0), Network::Testnet, 0.1).await;

    // Assert: kein Auftrag, aber das Buch zeigt die Boersenposition.
    assert!(result.is_ok());
    assert!(
        h.placed.lock().expect("placed").is_empty(),
        "ohne Signal darf kein Auftrag rausgehen"
    );
    let pfad = h.status_pfad.as_ref().expect("Statuspfad gesetzt");
    let roh = std::fs::read_to_string(pfad).expect("Status-Datei");
    let wert: serde_json::Value = serde_json::from_str(&roh).expect("Status ist JSON");
    assert_eq!(
        wert["net_quantity"], -0.4,
        "Buch muss die Boersenposition tragen, nicht den eigenen Zaehler: {roh}"
    );
}

/// Der Fehler vom 21.10.2026 im echten Ablauf: XPERP-Symbol mit Quote `USD`,
/// Konto fuehrt USDC. Mit der Quote bewertet der RiskGuard das Konto mit 0, misst
/// einen Drawdown von Millionen Prozent und lehnt JEDEN Auftrag ab. Mit der
/// Abrechnungswaehrung des Instruments geht derselbe Auftrag durch.
#[tokio::test]
async fn xperp_auftrag_wird_nicht_am_erfundenen_drawdown_abgelehnt() {
    // Arrange: Symbol sagt USD, Konto fuehrt USDC, Abrechnung ist USDC.
    // Die Attrappe wird je Kerze ZWEIMAL befragt: erst fuer das Buch, dann im
    // Risikopfad. Die Reihe ist deshalb so gelegt, dass der ERSTE Auftrag +0,05
    // uPnL sieht (das setzt den Peak) und der ZWEITE -0,50. Ohne die
    // Abrechnungswaehrung steht die Equity dann bei -0,50 gegen einen Peak von
    // 0,05 - der Guard misst 1100 % Drawdown und lehnt den zweiten Auftrag ab.
    let h = Harness::new("xperp-usdc", BUY_BELOW_40K)
        .with_symbol("SOL-USD_UM_XPERP-310404")
        .with_settle("USDC")
        .with_konto_waehrung("USDC")
        .with_position(0.1)
        .with_upl_reihe(&[0.0, 0.05, 0.0, -0.50])
        .with_status_file("xperp-usdc");

    // Act
    let result = h
        .run(&[35000.0, 36000.0], Some(46.43), Network::Testnet, 0.1)
        .await;

    // Assert: BEIDE Auftraege gehen raus, statt am erfundenen Drawdown zu scheitern.
    assert!(result.is_ok(), "Lauf scheiterte: {result:?}");
    let placed = h.placed.lock().expect("placed");
    assert_eq!(
        placed.len(),
        2,
        "beide Auftraege muessen durchgehen - sonst rechnet der Guard gegen eine leere Quote"
    );
}
