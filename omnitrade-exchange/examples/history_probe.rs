//! Loads recent candles from each exchange's public REST API and prints a summary.
//!
//! Usage: `cargo run -p omnitrade-exchange --example history_probe`

use omnitrade_exchange::{Exchange, Network};

#[tokio::main]
async fn main() {
    let mut failed = false;
    for exchange in Exchange::ALL {
        let symbol = exchange.default_symbol();
        match exchange
            .fetch_history(Network::Mainnet, symbol, "1m", 50)
            .await
        {
            Ok(candles) if !candles.is_empty() => {
                let first = &candles[0];
                let last = &candles[candles.len() - 1];
                let ascending = candles
                    .windows(2)
                    .all(|w| w[0].timestamp_ms < w[1].timestamp_ms);
                println!(
                    "{exchange} {symbol}: {} Kerzen, aufsteigend={ascending}, erste t={} letzte t={} close={}",
                    candles.len(), first.timestamp_ms, last.timestamp_ms, last.close
                );
                failed |= !ascending;
            }
            Ok(_) => {
                println!("{exchange} {symbol}: keine Kerzen");
                failed = true;
            }
            Err(e) => {
                println!("{exchange} {symbol}: FEHLER {e}");
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
