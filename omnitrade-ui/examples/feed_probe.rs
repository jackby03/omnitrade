//! Headless probe: runs the GUI feed for a few seconds and prints what arrived.
//!
//! Usage: `cargo run -p omnitrade-ui --example feed_probe -- [SYMBOL] [SECONDS] [EXCHANGE]`

use std::sync::{Arc, RwLock};
use std::time::Duration;

use omnitrade_exchange::{Exchange, Network};
use omnitrade_ui::account_feed::spawn_account_feed;
use omnitrade_ui::feed::spawn_feed;
use omnitrade_ui::UIState;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exchange: Exchange = args
        .get(2)
        .map_or(Ok(Exchange::Binance), |e| e.parse())
        .expect("exchange");
    let symbol = args
        .first()
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| exchange.default_symbol().to_string());
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);

    let state = Arc::new(RwLock::new(UIState::new()));
    if let Err(e) = spawn_feed(
        Arc::clone(&state),
        exchange,
        symbol.clone(),
        Network::Testnet,
    ) {
        eprintln!("Feed-Start fehlgeschlagen: {e}");
        std::process::exit(1);
    }
    if let Err(e) = spawn_account_feed(Arc::clone(&state), Network::Testnet) {
        eprintln!("Konto-Feed-Start fehlgeschlagen: {e}");
    }
    std::thread::sleep(Duration::from_secs(secs));

    let s = state.read().unwrap_or_else(|p| p.into_inner());
    println!("verbunden: {}", s.is_connected());
    println!("Kerzen: {}", s.latest_candles(&symbol, usize::MAX).len());
    if let Some(book) = s.orderbook.get(&symbol) {
        println!(
            "bestes Gebot: {:?}, bestes Angebot: {:?}",
            book.best_bid(),
            book.best_ask()
        );
    } else {
        println!("kein Orderbuch");
    }
    println!("Bestände: {}", s.balances.len());
    println!("USDT-Guthaben: {}", s.account_balance);
    println!("offene Orders: {}", s.open_orders.len());
    for line in &s.log {
        println!("log: {line}");
    }
    if !s.is_connected() {
        std::process::exit(1);
    }
}
