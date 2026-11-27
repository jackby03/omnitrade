//! Prints the trading rules (tick/step size, minimums) of Binance symbols.
//!
//! Usage: `cargo run -p omnitrade-exchange --example filters_probe -- [SYMBOL...]`
//! (public endpoint; uses the testnet).

use omnitrade_exchange::{fetch_symbol_filters, Network};

#[tokio::main]
async fn main() {
    let mut symbols: Vec<String> = std::env::args().skip(1).collect();
    if symbols.is_empty() {
        symbols = ["BTCUSDT", "ETHUSDT", "SHIBUSDT", "BNBUSDT"]
            .map(String::from)
            .to_vec();
    }
    let mut failed = false;
    for symbol in &symbols {
        match fetch_symbol_filters(Network::Testnet, symbol).await {
            Ok(f) => println!(
                "{symbol}: Preis {} Stellen, Menge {} Stellen, Mindestmenge {}, Mindestwert {}",
                f.price_decimals, f.qty_decimals, f.min_qty, f.min_notional
            ),
            Err(e) => {
                println!("{symbol}: FEHLER {e}");
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
