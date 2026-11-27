//! Probe against the public OKX WebSocket v5 feeds (no credentials needed).
//!
//! Usage: `cargo run -p omnitrade-exchange --example okx_probe -- [SYMBOL] [SECONDS] [demo|prod]`
//! Waits for one confirmed 1m candle (up to ~60 s) and counts depth messages meanwhile.

use std::time::Duration;

use omnitrade_exchange::{ExchangeStream, Network, OkxClient};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let symbol = args
        .first()
        .cloned()
        .unwrap_or_else(|| "BTC-USDT".to_string());
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(75);
    let network = match args.get(2).map(String::as_str) {
        Some("demo") => Network::Testnet,
        _ => Network::Mainnet,
    };

    let mut client = OkxClient::for_network(network);
    let mut candles = client
        .subscribe_candles(&symbol, "1m")
        .await
        .expect("candles");
    let mut depth = client.subscribe_depth(&symbol).await.expect("depth");
    client.connect().await.expect("connect");
    println!("verbunden ({network}), warte bis zu {secs} s auf eine bestätigte Kerze");

    let deadline = tokio::time::sleep(Duration::from_secs(secs));
    tokio::pin!(deadline);
    let (mut depth_msgs, mut closed) = (0usize, 0usize);
    loop {
        tokio::select! {
            Some(d) = depth.recv() => {
                depth_msgs += 1;
                if depth_msgs == 1 {
                    println!(
                        "erstes Depth-Paket: {} Gebote, {} Angebote, bestes Gebot {:?}",
                        d.bids.len(), d.asks.len(), d.bids.first()
                    );
                }
            }
            Some(c) = candles.recv() => {
                closed += 1;
                println!("bestätigte Kerze: t={} o={} h={} l={} c={} v={}",
                    c.timestamp_ms, c.open, c.high, c.low, c.close, c.volume);
                break;
            }
            () = &mut deadline => break,
        }
    }
    println!("Depth-Pakete: {depth_msgs}, bestätigte Kerzen: {closed}");
    let _ = client.disconnect().await;
    if depth_msgs == 0 || closed == 0 {
        std::process::exit(1);
    }
}
