//! Main application entry point for `omnitrade-ui`.
//!
//! Usage: `omnitrade-ui [SYMBOL] [testnet|mainnet] [--exchange binance|kraken|okx]`
//! (defaults: the exchange's default symbol, `testnet`, `binance`).
//! Market data is public; for Binance with credentials the account panel is filled
//! read-only. The GUI never places orders; its only action is the kill-switch flag file.

use std::sync::{Arc, RwLock};

use omnitrade_exchange::{Exchange, Network};
use omnitrade_ui::account_feed::spawn_account_feed;
use omnitrade_ui::feed::spawn_feed;
use omnitrade_ui::{OmniTradeApp, UIState};

/// Parsed command line.
#[derive(Debug, PartialEq)]
struct Options {
    exchange: Exchange,
    symbol: String,
    network: Network,
}

fn parse_network(arg: &str) -> Result<Network, String> {
    match arg {
        "testnet" => Ok(Network::Testnet),
        "mainnet" => Ok(Network::Mainnet),
        other => Err(format!("Unbekanntes Netzwerk '{other}' (testnet|mainnet)")),
    }
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut exchange = Exchange::Binance;
    let mut positional: Vec<&str> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--exchange" {
            let value = iter.next().ok_or("--exchange braucht einen Wert")?;
            exchange = value.parse()?;
        } else if arg.starts_with("--") {
            return Err(format!("Unbekannte Option '{arg}'"));
        } else {
            positional.push(arg);
        }
    }
    if positional.len() > 2 {
        return Err("Zu viele Argumente: [SYMBOL] [testnet|mainnet]".to_string());
    }
    if positional.iter().any(|a| a.trim().is_empty()) {
        return Err("Symbol und Netzwerk dürfen nicht leer sein".to_string());
    }
    let symbol = positional.first().map_or_else(
        || exchange.default_symbol().to_string(),
        |s| s.to_uppercase(),
    );
    let network = positional
        .get(1)
        .map_or(Ok(Network::Testnet), |n| parse_network(n))?;
    Ok(Options {
        exchange,
        symbol,
        network,
    })
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Options {
        exchange,
        symbol,
        network,
    } = match parse_args(&args) {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    let state = Arc::new(RwLock::new(UIState::new()));
    if let Err(e) = spawn_feed(Arc::clone(&state), exchange, symbol.clone(), network) {
        eprintln!("Feed konnte nicht gestartet werden: {e}");
        std::process::exit(1);
    }
    if exchange == Exchange::Binance {
        if let Err(e) = spawn_account_feed(Arc::clone(&state), network) {
            eprintln!("Konto-Feed konnte nicht gestartet werden: {e}");
        }
    } else {
        state
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .push_log(format!(
                "Konto-Anzeige gibt es nur für Binance (gewählt: {exchange})"
            ));
    }
    let status_path = omnitrade_ui::status_feed::resolve_path(None);
    omnitrade_ui::status_feed::spawn_poller(status_path, Arc::clone(&state));
    let mut app = OmniTradeApp::with_symbol(state, symbol);
    app.venue = exchange.to_string();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("omnitrade"),
        ..Default::default()
    };

    eframe::run_native("omnitrade", options, Box::new(|_cc| Ok(Box::new(app))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        parse_args(&args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn defaults_to_binance_testnet_with_default_symbol() {
        assert_eq!(
            parse(&[]),
            Ok(Options {
                exchange: Exchange::Binance,
                symbol: "BTCUSDT".into(),
                network: Network::Testnet
            })
        );
    }

    #[test]
    fn exchange_flag_picks_default_symbol_and_accepts_explicit_one() {
        let okx = parse(&["--exchange", "okx"]).expect("ok");
        assert_eq!(
            (okx.exchange, okx.symbol.as_str()),
            (Exchange::Okx, "BTC-USDT")
        );
        let kraken = parse(&["eth/usd", "mainnet", "--exchange", "kraken"]).expect("ok");
        assert_eq!(kraken.symbol, "ETH/USD");
        assert_eq!(kraken.network, Network::Mainnet);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(&["BTCUSDT", "foo"]).is_err());
        assert!(parse(&["--exchange"]).is_err());
        assert!(parse(&["--exchange", "bybit"]).is_err());
        assert!(parse(&["--frobnicate"]).is_err());
        assert!(parse(&[""]).is_err());
        assert!(parse(&["a", "testnet", "c"]).is_err());
    }
}
