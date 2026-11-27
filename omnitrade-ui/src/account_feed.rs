//! Read-only account poller: balances and open orders into [`UIState`].
//!
//! Needs API credentials (Keystore `omnitrade/binance/<network>`, fallback
//! `BINANCE_API_KEY`/`BINANCE_API_SECRET`). Without them the GUI keeps running with
//! market data only. This module never places or cancels orders.

use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use omnitrade_exchange::{ApiCredentials, BinanceRestClient, ExchangeTrading, Keystore, Network};

use crate::state::UIState;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

fn load_credentials(network: Network) -> Result<ApiCredentials, String> {
    if Keystore::credentials_exist("binance", network) {
        return Keystore::load_credentials("binance", network)
            .map_err(|e| format!("Keystore: {e}"));
    }
    match (
        std::env::var("BINANCE_API_KEY"),
        std::env::var("BINANCE_API_SECRET"),
    ) {
        (Ok(key), Ok(secret)) => Ok(ApiCredentials::new(key, secret, network)),
        _ => Err("keine Zugänge (Keystore oder BINANCE_API_KEY/BINANCE_API_SECRET)".to_string()),
    }
}

/// Starts the poller thread. Returns `Ok(None)` if no credentials are available.
pub fn spawn_account_feed(
    state: Arc<RwLock<UIState>>,
    network: Network,
) -> std::io::Result<Option<JoinHandle<()>>> {
    let credentials = match load_credentials(network) {
        Ok(c) => c,
        Err(reason) => {
            push_log(&state, format!("Konto-Anzeige aus: {reason}"));
            return Ok(None);
        }
    };
    std::thread::Builder::new()
        .name("omnitrade-account".to_string())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                push_log(&state, "Konto-Feed: Laufzeit nicht startbar".to_string());
                return;
            };
            runtime.block_on(poll_loop(state, credentials));
        })
        .map(Some)
}

async fn poll_loop(state: Arc<RwLock<UIState>>, credentials: ApiCredentials) {
    let client = match BinanceRestClient::new(credentials) {
        Ok(c) => c,
        Err(e) => {
            push_log(&state, format!("Konto-Feed: Client-Fehler: {e}"));
            return;
        }
    };
    let mut last_error = String::new();
    loop {
        let result = async {
            Ok::<_, omnitrade_exchange::ExchangeError>((
                client.get_balance().await?,
                client.get_open_orders().await?,
            ))
        }
        .await;
        match result {
            Ok((account, orders)) => {
                last_error.clear();
                let mut balances: Vec<(String, f64, f64)> = account
                    .balances
                    .into_iter()
                    .filter(|(_, b)| b.total() > 0.0)
                    .map(|(asset, b)| (asset, b.free, b.locked))
                    .collect();
                balances.sort_by_key(|(asset, _, _)| (asset_rank(asset), asset.clone()));
                let mut guard = state.write().unwrap_or_else(|p| p.into_inner());
                guard.set_account_snapshot(balances, orders);
            }
            Err(e) => {
                let msg = e.to_string();
                if msg != last_error {
                    push_log(&state, format!("Konto-Abruf fehlgeschlagen: {msg}"));
                    last_error = msg;
                }
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Main assets first so the balance list does not start with exotic testnet airdrops.
fn asset_rank(asset: &str) -> usize {
    ["USDT", "BTC", "ETH", "BNB"]
        .iter()
        .position(|a| *a == asset)
        .unwrap_or(usize::MAX)
}

fn push_log(state: &Arc<RwLock<UIState>>, line: String) {
    state
        .write()
        .unwrap_or_else(|p| p.into_inner())
        .push_log(line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_rank_puts_main_assets_first() {
        // Arrange
        let mut assets = vec!["0G", "BNB", "USDT", "ETH", "BTC", "1INCH"];

        // Act
        assets.sort_by_key(|a| (asset_rank(a), a.to_string()));

        // Assert
        assert_eq!(assets, ["USDT", "BTC", "ETH", "BNB", "0G", "1INCH"]);
    }
}
