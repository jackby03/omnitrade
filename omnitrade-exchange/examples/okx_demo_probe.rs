//! Probe of the signed OKX REST API in **demo trading** (play money only).
//!
//! Credentials from `OKX_API_KEY`, `OKX_API_SECRET`, `OKX_PASSPHRASE` (demo keys).
//! Usage: `cargo run -p omnitrade-exchange --example okx_demo_probe -- [balance|trade]`
//!
//! `balance` only reads the account. `trade` additionally runs a small round trip on the
//! demo account: far-from-market limit order -> cancel, then a market buy and sell back.

use omnitrade_core::Side;
use omnitrade_exchange::{ApiCredentials, ExchangeTrading, Network, OkxRestClient, OrderRequest};

const DEFAULT_SYMBOL: &str = "BTC-USDT";
const QTY: f64 = 0.001;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        eprintln!("{name} ist nicht gesetzt");
        std::process::exit(3);
    })
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map_or("balance", String::as_str);
    let symbol = args.get(1).map_or(DEFAULT_SYMBOL, String::as_str);
    let creds = ApiCredentials::new(
        env("OKX_API_KEY"),
        env("OKX_API_SECRET"),
        Network::Testnet, // Demo: sendet x-simulated-trading: 1, niemals echtes Geld
    );
    let mut client = OkxRestClient::new(creds, env("OKX_PASSPHRASE")).expect("client");
    // EEA/US-Demo-Konten liegen auf ihrer eigenen API-Domain, z. B. OKX_REST_BASE=https://eea.okx.com
    if let Ok(base) = std::env::var("OKX_REST_BASE") {
        println!("API-Domain: {base}");
        client = client.with_base_url(base);
    }

    let account = match client.get_balance().await {
        Ok(a) => a,
        Err(e) => {
            println!("Kontostand (Demo) fehlgeschlagen: {e}");
            std::process::exit(1);
        }
    };
    let usdt = account.balances.get("USDT").map_or(0.0, |b| b.total());
    println!(
        "Demo-Konto lesbar: {} Assets, USDT {usdt:.2}",
        account.balances.len()
    );
    let mut held: Vec<_> = account
        .balances
        .iter()
        .filter(|(_, b)| b.total() > 0.0)
        .collect();
    held.sort_by(|a, b| a.0.cmp(b.0));
    for (ccy, b) in held {
        println!("  {ccy}: frei {:.8}, gesperrt {:.8}", b.free, b.locked);
    }
    if mode != "trade" {
        return;
    }

    // 1) Limit-Order weit unter dem Markt anlegen, abfragen, stornieren.
    let limit = OrderRequest::limit(symbol.into(), Side::Buy, QTY, 10_000.0);
    let order = client.place_order(&limit).await.expect("limit order");
    println!("Limit-Order angelegt: id={}", order.id);
    let open = client.get_open_orders().await.expect("open orders");
    println!(
        "offene Orders: {} (enthält die neue: {})",
        open.len(),
        open.iter().any(|o| o.id == order.id)
    );
    client
        .cancel_order(&order.id.to_string(), symbol)
        .await
        .expect("cancel");
    let after = client
        .get_order(&order.id.to_string(), symbol)
        .await
        .expect("get_order");
    println!("nach Storno: {:?}", after.map(|o| o.status));

    // 2) Market-Kauf und Rückverkauf.
    let buy = client
        .place_order(&OrderRequest::market(symbol.into(), Side::Buy, QTY))
        .await
        .expect("market buy");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let filled = client
        .get_order(&buy.id.to_string(), symbol)
        .await
        .expect("get_order");
    println!(
        "Market-Kauf: id={} Status={:?} gefüllt={:?}",
        buy.id,
        filled.as_ref().map(|o| o.status),
        filled.as_ref().map(|o| o.filled_quantity)
    );
    let sell = client
        .place_order(&OrderRequest::market(symbol.into(), Side::Sell, QTY))
        .await
        .expect("market sell");
    println!("Rückverkauf angelegt: id={}", sell.id);
}
