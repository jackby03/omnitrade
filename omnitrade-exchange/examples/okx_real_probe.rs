//! Bounded **real-money** round trip on OKX production (BTC-USDT or BTC-USDC spot).
//!
//! Hard cap: 20 USD-stablecoin notional. The program only trades when started with
//! `run --confirm-real-money`; `check` and `balances` are read-only.
//!
//! Credentials from `OKX_API_KEY`, `OKX_API_SECRET`, `OKX_PASSPHRASE` (production keys);
//! optional `OKX_REST_BASE` for regional domains (`https://eea.okx.com`, `https://us.okx.com`).
//! Usage: `cargo run -p omnitrade-exchange --example okx_real_probe -- check`
//!        `cargo run -p omnitrade-exchange --example okx_real_probe -- run --confirm-real-money`
//!
//! `run` buys about 18 USDT of BTC at market, waits for the fill, then sells back exactly
//! the BTC amount that arrived (the fee is taken in BTC), so no position stays open.

use std::time::Duration;

use omnitrade_core::{OrderStatus, Side};
use omnitrade_exchange::{
    Account, ApiCredentials, Exchange, ExchangeTrading, Network, OkxRestClient, OrderRequest,
};

/// Default pair; EEA accounts usually hold USDC instead of USDT (`--symbol BTC-USDC`).
const DEFAULT_SYMBOL: &str = "BTC-USDT";
/// Absolute notional cap in USDT. Never raised by arguments.
const MAX_NOTIONAL_USDT: f64 = 20.0;
/// Buy target, below the cap to leave room for price movement between quote and fill.
const TARGET_NOTIONAL_USDT: f64 = 18.0;
const MIN_BTC_ORDER: f64 = 0.00001;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        eprintln!("{name} ist nicht gesetzt");
        std::process::exit(3);
    })
}

fn free(account: &Account, ccy: &str) -> f64 {
    account.balances.get(ccy).map_or(0.0, |b| b.free)
}

fn floor_to(value: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (value * f).floor() / f
}

async fn wait_until_filled(
    client: &OkxRestClient,
    symbol: &str,
    id: u64,
    what: &str,
) -> Result<f64, String> {
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match client.get_order(&id.to_string(), symbol).await {
            Ok(Some(o)) if o.status == OrderStatus::Filled => {
                println!("{what}: gefüllt, Menge {}", o.filled_quantity);
                return Ok(o.filled_quantity);
            }
            Ok(Some(o)) if matches!(o.status, OrderStatus::Cancelled | OrderStatus::Rejected) => {
                return Err(format!("{what}: Status {:?}", o.status));
            }
            Ok(_) => {}
            Err(e) => return Err(format!("{what}: Abfrage fehlgeschlagen: {e}")),
        }
    }
    Err(format!("{what}: nach 10 s nicht gefüllt (id {id})"))
}

/// Limit buy far below the market (cannot fill), verify it via `get_open_orders` and
/// `get_order`, cancel exactly that order, verify again. Other orders on the account are
/// only counted, never touched. Notional stays a few USD, far below the cap.
async fn cancel_test(client: &OkxRestClient, symbol: &str, price: f64, confirmed: bool) {
    const QTY: f64 = 0.0001;
    if !confirmed {
        println!("Abbruch: ohne --confirm-real-money wird keine Order angelegt.");
        std::process::exit(2);
    }
    let open_before = client.get_open_orders().await.unwrap_or_else(|e| {
        println!("get_open_orders fehlgeschlagen: {e}");
        std::process::exit(1);
    });
    println!("Offene Orders im Konto vorher: {}", open_before.len());

    let limit_price = floor_to(price * 0.5, 1);
    let notional = QTY * limit_price;
    if notional > MAX_NOTIONAL_USDT {
        println!("Abbruch: Notional {notional:.2} über dem Deckel.");
        std::process::exit(2);
    }
    let order = match client
        .place_order(&OrderRequest::limit(
            symbol.into(),
            Side::Buy,
            QTY,
            limit_price,
        ))
        .await
    {
        Ok(o) => o,
        Err(e) => {
            println!("Limit-Order abgelehnt: {e} (nichts angelegt)");
            std::process::exit(1);
        }
    };
    println!(
        "Limit-Buy angelegt: id={} Menge {QTY} @ {limit_price} (~{notional:.2}, Markt {price:.1})",
        order.id
    );

    let id = order.id.to_string();
    let listed = client
        .get_open_orders()
        .await
        .map(|o| o.iter().any(|x| x.id == order.id));
    println!("In get_open_orders enthalten: {listed:?}");
    let status = client
        .get_order(&id, symbol)
        .await
        .map(|o| o.map(|o| o.status));
    println!("get_order vor Storno: {status:?}");

    let cancelled = client.cancel_order(&id, symbol).await;
    println!("cancel_order: {cancelled:?}");
    if cancelled.is_err() {
        println!("ACHTUNG: Storno fehlgeschlagen, Order {id} bitte manuell stornieren!");
        std::process::exit(1);
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let status = client
        .get_order(&id, symbol)
        .await
        .map(|o| o.map(|o| o.status));
    println!("get_order nach Storno: {status:?}");
    let open_after = client.get_open_orders().await.unwrap_or_default();
    println!(
        "Offene Orders nachher: {} (neue Order noch enthalten: {})",
        open_after.len(),
        open_after.iter().any(|x| x.id == order.id)
    );
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map_or("check", String::as_str);
    let confirmed = args.iter().any(|a| a == "--confirm-real-money");
    let symbol = args
        .iter()
        .position(|a| a == "--symbol")
        .and_then(|i| args.get(i + 1))
        .map_or(DEFAULT_SYMBOL, String::as_str);
    let Some((base, quote)) = symbol.split_once('-') else {
        println!("Symbol muss BASE-QUOTE sein, z. B. BTC-USDC");
        std::process::exit(2);
    };
    if base != "BTC" || !matches!(quote, "USDT" | "USDC") {
        println!("Nur BTC gegen USDT/USDC erlaubt (Deckel gilt für USD-Stablecoins).");
        std::process::exit(2);
    }

    let creds = ApiCredentials::new(env("OKX_API_KEY"), env("OKX_API_SECRET"), Network::Mainnet);
    let mut client = OkxRestClient::new(creds, env("OKX_PASSPHRASE")).expect("client");
    // EEA/US accounts live on their own API domain, e.g. OKX_REST_BASE=https://eea.okx.com
    if let Ok(base) = std::env::var("OKX_REST_BASE") {
        println!("API-Domain: {base}");
        client = client.with_base_url(base);
    }

    let before = client.get_balance().await.unwrap_or_else(|e| {
        println!("Kontostand fehlgeschlagen: {e}");
        std::process::exit(1);
    });
    if mode == "balances" {
        let mut held: Vec<_> = before
            .balances
            .iter()
            .filter(|(_, b)| b.total() > 0.0)
            .collect();
        held.sort_by(|a, b| a.0.cmp(b.0));
        println!("Assets mit Bestand: {}", held.len());
        for (ccy, b) in held {
            println!("  {ccy}: frei {:.8}, gesperrt {:.8}", b.free, b.locked);
        }
        return;
    }
    let (usdt0, btc0) = (free(&before, quote), free(&before, "BTC"));
    println!("Konto lesbar. Frei: {quote} {usdt0:.4}, BTC {btc0:.8}");

    let price = Exchange::Okx
        .fetch_history(Network::Mainnet, symbol, "1m", 5)
        .await
        .ok()
        .and_then(|c| c.last().map(|c| c.close))
        .unwrap_or_else(|| {
            println!("Kein Referenzpreis verfügbar");
            std::process::exit(1);
        });
    let qty = floor_to(TARGET_NOTIONAL_USDT / price, 5);
    println!(
        "{symbol}: Referenzpreis {price:.2}, geplante Menge {qty:.5} BTC (~{:.2} {quote}, Deckel {MAX_NOTIONAL_USDT})",
        qty * price
    );

    if mode == "cancel-test" {
        cancel_test(&client, symbol, price, confirmed).await;
        return;
    }
    if mode != "run" {
        println!("Nur Prüfung (check): keine Order abgesetzt.");
        return;
    }
    if !confirmed {
        println!("Abbruch: ohne --confirm-real-money wird nicht gehandelt.");
        std::process::exit(2);
    }
    if qty < MIN_BTC_ORDER || qty * price > MAX_NOTIONAL_USDT {
        println!("Abbruch: Menge außerhalb der Grenzen.");
        std::process::exit(2);
    }
    if usdt0 < qty * price * 1.01 {
        println!(
            "Abbruch: zu wenig freies {quote} ({usdt0:.4}) für {:.2} {quote}.",
            qty * price
        );
        std::process::exit(2);
    }

    // 1) Market-Kauf.
    let buy = client
        .place_order(&OrderRequest::market(symbol.into(), Side::Buy, qty))
        .await
        .unwrap_or_else(|e| {
            println!("Kauf abgelehnt: {e}");
            std::process::exit(1);
        });
    println!("Market-Kauf angelegt: id={}", buy.id);
    let bought = match wait_until_filled(&client, symbol, buy.id, "Kauf").await {
        Ok(filled) => filled,
        Err(e) => {
            println!("{e}");
            println!("KEIN Rückverkauf ausgelöst. Bitte Konto prüfen.");
            std::process::exit(1);
        }
    };

    // 2) Tatsächlich erhaltene BTC-Menge (Gebühr wird in BTC abgezogen) zurückverkaufen.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let mid = client.get_balance().await.expect("balance after buy");
    // Never sell more than this purchase filled: other orders on the account may move
    // balances at the same time.
    let received = floor_to((free(&mid, "BTC") - btc0).min(bought), 8);
    println!("Erhaltene BTC (netto nach Gebühr): {received:.8}");
    if received < MIN_BTC_ORDER {
        println!(
            "Zu wenig BTC zum Rückverkauf (Mindestmenge {MIN_BTC_ORDER}). Bitte Konto prüfen."
        );
        std::process::exit(1);
    }
    let sell = client
        .place_order(&OrderRequest::market(symbol.into(), Side::Sell, received))
        .await
        .unwrap_or_else(|e| {
            println!("Rückverkauf abgelehnt: {e}. BTC bleiben im Konto!");
            std::process::exit(1);
        });
    println!("Market-Verkauf angelegt: id={}", sell.id);
    if let Err(e) = wait_until_filled(&client, symbol, sell.id, "Verkauf").await {
        println!("{e}. BTC könnten noch im Konto sein!");
        std::process::exit(1);
    }

    tokio::time::sleep(Duration::from_secs(1)).await;
    let after = client.get_balance().await.expect("balance after sell");
    let (usdt1, btc1) = (free(&after, quote), free(&after, "BTC"));
    println!(
        "Ergebnis: {quote} {usdt0:.4} -> {usdt1:.4} (Differenz {:+.4}), BTC {btc0:.8} -> {btc1:.8}",
        usdt1 - usdt0
    );
}
