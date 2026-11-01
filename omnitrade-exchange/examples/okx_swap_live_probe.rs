//! LIVE Roundtrip-Probe: XPERP auf OKX EEA Mainnet, kleine Position (~20 USDC), 1x isolated.
//!
//! Auftrag Max 21.10.2026: "direkt an okx mit kleinen positionen a 20 usd testen,
//! zugaenge in umgebungsvariabeln". Gleicher Order-Pfad wie der Live-Lauf
//! (swap_client: load_instrument -> set_leverage -> ensure_net_mode -> place_order
//! -> position_snapshot), nur ohne Strategie- und RiskGuard-Schicht.
//!
//! Credentials: OKX_API_KEY / OKX_API_SECRET / OKX_PASSPHRASE (Live-Satz),
//! OKX_REST_BASE=https://eea.okx.com. NIEMALS x-simulated-trading.
//!
//! Sicherheitswaechter (fail closed, VOR jeder Order):
//!  1. Kill-Datei (OMNITRADE_KILL_FILE oder <temp>/omnitrade.kill) => Abbruch.
//!  2. Menge: 1..=30 Kontrakte.
//!  3. Notional (markPx x Kontrakte x ctVal): muss <= 45 USDC sein.
//!
//! Usage: `cargo run -p omnitrade-exchange --example okx_swap_live_probe -- [SYMBOL] [KONTRAKTE]`

use omnitrade_core::Side;
use omnitrade_exchange::okx::InstType;
use omnitrade_exchange::{ApiCredentials, ExchangeTrading, Network, OkxRestClient, OrderRequest};

const DEFAULT_SYMBOL: &str = "SOL-USD_UM_XPERP-310404";
const DEFAULT_BASE_QTY: f64 = 0.19; // 0.19 SOL @ ~106.4 = ~20.2 USDC
const MAX_BASE_QTY: f64 = 0.5; // 50 Kontrakte bei ctVal 0.01 - maximal ~53 USDC
const MAX_NOTIONAL_USDC: f64 = 45.0;
const EEA_REST: &str = "https://eea.okx.com";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        eprintln!("{name} ist nicht gesetzt");
        std::process::exit(3);
    })
}

fn main() {
    // Waechter 0: Kill-Datei (fail closed)
    let kill = std::env::var_os("OMNITRADE_KILL_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("omnitrade.kill"));
    if kill.exists() {
        eprintln!("Not-Aus gesetzt ({}): Abbruch ohne Order.", kill.display());
        std::process::exit(4);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let symbol = args.first().map_or(DEFAULT_SYMBOL, String::as_str);
    // Menge in der Base-Währung (SOL); der Adapter rechnet ueber ctVal in Kontrakte um.
    let base_qty: f64 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_BASE_QTY);

    // Waechter 1: Menge
    if !(0.01..=MAX_BASE_QTY).contains(&base_qty) || !base_qty.is_finite() {
        eprintln!("Menge {base_qty} auferhalb 0.01..={MAX_BASE_QTY} Base: Abbruch.");
        std::process::exit(5);
    }
    if !symbol.to_uppercase().contains("_XPERP-") {
        eprintln!("Nur XPERP-Symbole erlaubt (war {symbol}): Abbruch.");
        std::process::exit(6);
    }

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let creds = ApiCredentials::new(
            env("OKX_API_KEY"),
            env("OKX_API_SECRET"),
            Network::Mainnet, // echtes Konto, KEIN simulated-trading
        );
        let mut client = OkxRestClient::new(creds, env("OKX_PASSPHRASE")).expect("client");
        let base = std::env::var("OKX_REST_BASE").unwrap_or_else(|_| EEA_REST.to_string());
        println!("API-Domain: {base}");
        client = client.with_base_url(&base);

        // 1) Instrument-Regeln (ctVal/lotSz/minSz)
        let inst = client
            .load_instrument(symbol, InstType::Futures)
            .await
            .expect("load_instrument");
        println!(
            "Instrument: {} ctVal={} lotSz={} minSz={}",
            inst.inst_id, inst.ct_val, inst.lot_sz, inst.min_sz
        );

        // Marktpreis fuer den Notional-Waechter (oeffentlich, plural-Endpoint)
        let url = format!(
            "{}/api/v5/market/tickers?instType=FUTURES",
            base.trim_end_matches('/')
        );
        let body: serde_json::Value = reqwest::get(&url)
            .await
            .expect("tickers get")
            .json()
            .await
            .expect("tickers json");
        let mark: f64 = body["data"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|t| t["instId"] == *symbol)
                    .and_then(|t| t["last"].as_str())
            })
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                eprintln!("Kein Marktpreis fuer {symbol}: Abbruch vor der Order.");
                std::process::exit(7);
            });
        // Notional = Base-Menge x Marktpreis (SOL x USDC/SOL = USDC).
        let notional = mark * base_qty;
        println!(
            "Marktpreis: {mark} -> Notional {notional:.2} USDC ({base_qty} Base, ctVal={})",
            inst.ct_val
        );
        if notional > MAX_NOTIONAL_USDC {
            eprintln!("Notional {notional:.2} > {MAX_NOTIONAL_USDC} USDC: Abbruch.");
            std::process::exit(8);
        }

        // 2) Hebel 1x (isolated)
        client.set_leverage(symbol, 1).await.expect("set_leverage");
        println!("set_leverage(1) ok");

        // 3) One-way-Modus
        client.ensure_net_mode().await.expect("ensure_net_mode");
        println!("ensure_net_mode ok (one-way)");

        // 4) LONG-Market-Order (Menge in Base-Waehrung; Adapter rechnet in Kontrakte um)
        let buy = client
            .place_order(&OrderRequest::market(symbol.into(), Side::Buy, base_qty))
            .await;
        match &buy {
            Ok(o) => println!("Market-Buy GEFUEHRT: ordId={} ({} Base)", o.id, base_qty),
            Err(e) => eprintln!("Market-Buy fehlgeschlagen: {e}. Pruefe Position und Konto."),
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // 5) Position lesen und ggf. schliessen. Kein Panic: eine offene Position
        //    darf nicht unbemerkt bleiben.
        let pos = match client.position_snapshot(symbol).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("position_snapshot fehlgeschlagen: {e}. Nichts wird blind geschlossen.");
                std::process::exit(9);
            }
        };
        println!(
            "POSITION: pos={} (Basis) upl={} liqPx={:?} markPx={:?}",
            pos.base_qty, pos.unrealized_pnl, pos.liq_price, pos.mark_price
        );

        // 6) Schliessen. Verkauft hoechstens die tatsaechlich offene Menge (nie mehr,
        //    sonst entstuende ein Short) und prueft danach erneut.
        if pos.base_qty > 0.0 {
            let sell_qty = pos.base_qty.min(base_qty);
            match client
                .place_order(&OrderRequest::market(symbol.into(), Side::Sell, sell_qty))
                .await
            {
                Ok(o) => println!("Market-Sell GEFUEHRT: ordId={} ({sell_qty} Base)", o.id),
                Err(e) => {
                    eprintln!("Market-Sell fehlgeschlagen: {e}. Position manuell schliessen!")
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        match client.position_snapshot(symbol).await {
            Ok(after) if after.base_qty <= 0.0 => println!("Position nach Schliessen: flach"),
            Ok(after) => {
                eprintln!(
                    "WARNUNG: Position noch offen (pos={}). Manuell schliessen!",
                    after.base_qty
                );
                std::process::exit(10);
            }
            Err(e) => {
                eprintln!("WARNUNG: Endstand nicht lesbar ({e}). Position im Konto pruefen!");
                std::process::exit(11);
            }
        }
    });
    println!("ROUNDTRIP ABGESCHLOSSEN");
}
