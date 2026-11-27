//! Live trading command handler.
//!
//! Orchestrates live trading setup: initializes exchange connections,
//! instantiates RiskGuard, and delegates to the live_loop.

use crate::grid_loop::{grid_loop, GridLiveConfig};
use crate::kill_switch;
use crate::live_loop::{live_loop, valid_mainnet_cap, LiveConfig, MAINNET_HARD_CAP};
use crate::status::StatusWriter;
use omnitrade_exchange::okx::{swap, InstType};
use omnitrade_exchange::{
    credentials, fetch_symbol_filters, BinanceClient, Exchange, Keystore, Network, OkxClient,
    OkxRestClient,
};
use omnitrade_risk::guard::RiskGuard;
use omnitrade_risk::limits::RiskLimits;
use omnitrade_script::GridConfig;
use std::path::Path;
use std::time::Duration;
use tracing::{info, warn};

/// Abstand der Fill-Abfragen im Grid-Betrieb.
const GRID_POLL_SECS: u64 = 5;

/// Parameter des `live`-Befehls.
#[derive(Debug, Clone, Copy)]
pub struct LiveArgs<'a> {
    /// Strategieskript (`--script`).
    pub script_path: Option<&'a Path>,
    /// Grid-Konfiguration (`--grid-config`).
    pub grid_config_path: Option<&'a Path>,
    /// Handelssymbol in der Schreibweise der Börse.
    pub symbol: &'a str,
    /// Börse: `binance`, `okx` oder `kraken`.
    pub exchange: &'a str,
    /// `testnet` oder `mainnet`.
    pub network: &'a str,
    /// Referenzkapital (nur Anzeige, die Limits messen am echten Konto).
    pub initial_capital: f64,
    /// Menge je Market-Order in der Basiswährung.
    pub order_quantity: f64,
    /// Deckel für die vom Lauf aufgebaute Position (Quote-Währung); auf Mainnet Pflicht.
    pub max_notional_usd: Option<f64>,
    /// Ausdrückliche Bestätigung, dass auf Mainnet echtes Geld eingesetzt wird.
    pub confirm_real_money: bool,
    /// Erlaubt Kurzpositionen (Sell bei flacher Position öffnet Short).
    /// Standard ist long-only (Haussatz C.2).
    pub allow_shorts: bool,
    /// JSON-Status-Snapshot für Beobachter (`--status-file`).
    pub status_file: Option<&'a Path>,
    /// Kerzenlänge der Strategie (`--interval`), z. B. "1m", "1h".
    pub interval: &'a str,
}

/// Anzahl abgeschlossener Kerzen, mit denen die Indikatoren vorgewärmt werden.
const WARMUP_CANDLES: usize = 300;

/// Handles the live trading command.
///
/// Currently supports script-based trading only.
/// Grid-Betrieb (`--grid-config`) läuft nur auf dem Testnet.
/// Sets up exchange connections and starts the live trading event loop.
/// Returns error if setup fails; the live loop itself handles individual
/// order placement errors gracefully.
///
/// # Errors
/// Returns error if network parsing fails, file I/O fails,
/// or Binance connection/credential issues occur.
pub async fn handle_live(args: &LiveArgs<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let LiveArgs {
        script_path,
        grid_config_path,
        symbol,
        exchange: exchange_str,
        network: network_str,
        initial_capital,
        order_quantity,
        max_notional_usd: max_notional,
        confirm_real_money,
        allow_shorts,
        status_file,
        interval,
    } = *args;

    // Validate that either script or grid is specified
    if script_path.is_none() && grid_config_path.is_none() {
        return Err("Error: either --script or --grid-config must be provided".into());
    }

    let exchange: Exchange = exchange_str.parse()?;
    if exchange == Exchange::Kraken {
        return Err(
            "Kraken: Handel ist gesperrt (Spot hat kein Testnet). Marktdaten gibt es in der GUI."
                .into(),
        );
    }
    if grid_config_path.is_some() && exchange != Exchange::Binance {
        return Err("Grid-Live läuft nur auf Binance (feste Tick-/Step-Size-Rundung)".into());
    }

    // Parse network string
    let network = match network_str {
        "testnet" => Network::Testnet,
        "mainnet" => Network::Mainnet,
        other => {
            return Err(format!("Invalid network: {}. Use 'testnet' or 'mainnet'", other).into())
        }
    };

    if network == Network::Mainnet {
        if !confirm_real_money {
            return Err(
                "Mainnet setzt echtes Geld ein: --confirm-real-money ist erforderlich".into(),
            );
        }
        if exchange != Exchange::Okx {
            return Err(
                "Mainnet-Handel ist nur für OKX freigegeben (nur dort am echten Konto geprüft)"
                    .into(),
            );
        }
        if grid_config_path.is_some() {
            return Err("Grid-Live auf Mainnet ist gesperrt (nur Testnet)".into());
        }
        if is_contract_symbol(symbol) {
            // Perpetuals bleiben auf Mainnet gesperrt: Der Live-Pfad uebergibt keine
            // Stop-Loss-Orders an die Boerse, und eine perpspezifische Notional-
            // Obergrenze fehlt. Freigabe erst durch Max, wenn beides umgesetzt ist.
            return Err(
                "SWAP/XPERP auf Mainnet ist gesperrt: Stop-Loss-Orders an die Boerse \
                 und perpspezifische Notional-Obergrenze fehlen noch (Freigabe durch Max ausstehend)"
                    .into(),
            );
        }
        if !valid_mainnet_cap(max_notional) {
            return Err(format!(
                "Mainnet braucht --max-notional-usd größer 0 und höchstens {MAINNET_HARD_CAP}"
            )
            .into());
        }
    }
    if let Some(cap) = max_notional {
        if !cap.is_finite() || cap <= 0.0 {
            return Err(format!("--max-notional-usd muss positiv sein (war {cap})").into());
        }
    }

    // Mainnet safety gate: require explicit environment variable
    if network == Network::Mainnet && std::env::var("OMNITRADE_MAINNET_APPROVED").is_err() {
        return Err(
            "Error: Mainnet trading requires OMNITRADE_MAINNET_APPROVED environment variable"
                .into(),
        );
    }

    // Gesetzter Not-Aus verhindert den Start (fail closed).
    let kill_file = kill_switch::default_path();
    if kill_switch::is_engaged(&kill_file) {
        return Err(format!(
            "Not-Aus ist gesetzt ({}). Aufheben mit: omnitrade-cli kill --reset",
            kill_file.display()
        )
        .into());
    }

    // Initialize risk guard with appropriate limits
    let mut risk_guard = match (network, max_notional) {
        // Mainnet: Limits skalieren mit dem Deckel (Verlust 25 %, Drawdown 10 %).
        (Network::Mainnet, Some(cap)) => RiskGuard::new(RiskLimits::new(cap, 10.0, cap * 0.25)),
        (Network::Mainnet, None) => RiskGuard::mainnet(),
        (Network::Testnet, _) => RiskGuard::testnet(),
    }
    // Verlustlimit gegen das echte Eigenkapital zu Laufbeginn messen, nicht gegen
    // das angegebene Startkapital.
    .with_run_baseline();

    info!("Starting live trading on {} (symbol: {})", network, symbol);
    info!("Initial capital: {}", initial_capital);

    // Handle script-based trading
    if let Some(script_path) = script_path {
        info!("Using strategy script: {}", script_path.display());
        info!("Script-based trading mode enabled");
        if network == Network::Mainnet {
            tracing::warn!(
                "ECHTES GELD: Mainnet auf {exchange}, Deckel {} (Höchstwert {MAINNET_HARD_CAP})",
                max_notional.unwrap_or(0.0)
            );
        }

        let script_str = script_path.to_string_lossy().to_string();
        // Vorwärmung: abgeschlossene Kerzen der Börse, damit Indikatoren sofort greifen.
        let warmup = exchange
            .fetch_history(network, symbol, interval, WARMUP_CANDLES)
            .await
            .map_err(|e| format!("Vorwärmung für {symbol} ({interval}) fehlgeschlagen: {e}"))?;
        info!(
            "Vorwärmung: {} abgeschlossene {interval}-Kerzen",
            warmup.len()
        );
        let mut status = StatusWriter::new(
            status_file.map(Path::to_path_buf),
            exchange_str,
            symbol,
            network_str,
            &script_str,
            initial_capital,
        );
        let config = LiveConfig {
            script_path: &script_str,
            symbol,
            network,
            initial_capital,
            order_quantity,
            kill_file: &kill_file,
            max_notional,
            interval,
            warmup: &warmup,
            allow_shorts,
        };

        match exchange {
            Exchange::Binance => {
                let creds = load_credentials(network)?;
                let trading_creds = creds.clone();

                // Two clients, one per role: the stream client owns the WebSocket
                // connection (connect() takes &mut self), the trading client places
                // orders (takes &self). BinanceClient is not Clone, so each role gets
                // its own instance built from the same credentials.
                let mut stream_client = BinanceClient::for_network(network);
                stream_client.set_credentials(creds)?;
                let mut trading_client = BinanceClient::new();
                trading_client.set_credentials(trading_creds)?;

                let outcome = live_loop(
                    &config,
                    None,
                    stream_client,
                    trading_client,
                    &mut risk_guard,
                    &mut status,
                )
                .await;
                finish_status(&mut status, &outcome);
                outcome?;
            }
            Exchange::Okx => {
                // Testnet = OKX demo trading (demo API keys, simulated-trading header).
                let eea = okx_region_is_eea()?;
                let (creds, passphrase) = load_okx_credentials(network)?;
                let stream_client = OkxClient::for_region(network, eea);
                let mut trading_client = OkxRestClient::new(creds, passphrase)?;
                // EWR-Konten: eea.okx.com. OKX_REST_BASE überschreibt den Default.
                if eea {
                    trading_client = trading_client.with_base_url(OKX_EEA_REST);
                }
                if let Ok(base) = std::env::var("OKX_REST_BASE") {
                    info!("OKX-API-Domain: {base}");
                    trading_client = trading_client.with_base_url(base);
                }
                // Abrechnungswaehrung des Kontrakts (XPERP: USDC) - sie ist die
                // Grundlage der Equity-Bewertung im RiskGuard.
                let settle = if is_contract_symbol(symbol) {
                    prepare_okx_swap(&trading_client, symbol).await?
                } else {
                    None
                };

                let outcome = live_loop(
                    &config,
                    settle.as_deref(),
                    stream_client,
                    trading_client,
                    &mut risk_guard,
                    &mut status,
                )
                .await;
                finish_status(&mut status, &outcome);
                outcome?;
            }
            Exchange::Kraken => return Err("Kraken: Handel ist gesperrt".into()),
        }

        info!("Live trading loop completed gracefully");
    } else if let Some(grid_path) = grid_config_path {
        info!("Grid trading mode: {}", grid_path.display());

        // Wie die Skript-Schleife: Mainnet bleibt gesperrt, bis Phase-3-Gates stehen.
        if network == Network::Mainnet {
            return Err("Grid-Live auf Mainnet ist gesperrt (nur Testnet)".into());
        }

        let grid = GridConfig::from_json(&std::fs::read_to_string(grid_path)?)
            .map_err(|e| format!("Grid-Konfiguration ungültig: {e}"))?;

        // Rundung aus den echten Handelsregeln des Symbols (fail closed bei Fehler).
        let filters = fetch_symbol_filters(network, symbol)
            .await
            .map_err(|e| format!("Handelsregeln für {symbol} nicht ladbar: {e}"))?;
        info!(
            "Handelsregeln {symbol}: {} Preis-, {} Mengen-Nachkommastellen, Mindestmenge {}, Mindestwert {}",
            filters.price_decimals, filters.qty_decimals, filters.min_qty, filters.min_notional
        );

        let mut trading_client = BinanceClient::new();
        trading_client.set_credentials(load_credentials(network)?)?;

        let config = GridLiveConfig {
            grid: &grid,
            symbol,
            initial_capital,
            kill_file: &kill_file,
            poll_interval: Duration::from_secs(GRID_POLL_SECS),
            price_decimals: filters.price_decimals,
            qty_decimals: filters.qty_decimals,
        };
        grid_loop(&config, trading_client, &mut risk_guard).await?;

        info!("Grid-Lauf beendet");
    }

    Ok(())
}

/// Zugänge: Keystore (getrennt je Netzwerk) hat Vorrang, Umgebungsvariablen
/// nur als Fallback für Läufe ohne Credential Manager.
fn load_credentials(
    network: Network,
) -> Result<credentials::ApiCredentials, Box<dyn std::error::Error>> {
    let creds = if Keystore::credentials_exist("binance", network) {
        Keystore::load_credentials("binance", network).map_err(|e| format!("Keystore: {e}"))?
    } else {
        let api_key = std::env::var("BINANCE_API_KEY").map_err(|_| {
            "Keine Zugänge: weder Keystore-Eintrag omnitrade/binance/<netzwerk> noch BINANCE_API_KEY"
        })?;
        let api_secret = std::env::var("BINANCE_API_SECRET")
            .map_err(|_| "BINANCE_API_SECRET environment variable not set")?;
        credentials::ApiCredentials::new(api_key, api_secret, network)
    };
    Ok(creds)
}

/// Setzt den Endzustand im Status-Snapshot und schreibt ihn; ein Schreibfehler stoppt nichts.
fn finish_status<E: std::fmt::Display>(status: &mut StatusWriter, outcome: &Result<(), E>) {
    match outcome {
        Ok(()) => status.set_state("stopped"),
        Err(e) => status.set_error(&e.to_string()),
    }
    if let Err(e) = status.flush() {
        tracing::warn!("Status-Datei konnte nicht geschrieben werden: {e}");
    }
}

/// REST-Domain für EWR-Konten.
const OKX_EEA_REST: &str = "https://eea.okx.com";

/// `OKX_REGION=eea` für EWR-Konten; `global` oder ungesetzt sonst. Andere Werte
/// werden abgelehnt, damit ein Tippfehler nicht still auf die falsche Domain zeigt.
fn okx_region_is_eea() -> Result<bool, Box<dyn std::error::Error>> {
    match std::env::var("OKX_REGION").ok().as_deref() {
        None | Some("global") => Ok(false),
        Some("eea") => Ok(true),
        Some(other) => {
            Err(format!("OKX_REGION muss 'eea' oder 'global' sein, war '{other}'").into())
        }
    }
}

/// Erkennt kontraktbasierte OKX-Symbole: SWAP (`BTC-USDT-SWAP`), XPERP
/// (`BTC-USD_UM_XPERP-310404`) und datierte Futures (`BTC-USD_UM-261030`).
/// Die Regel steht EINMAL im Exchange-Crate.
fn is_contract_symbol(symbol: &str) -> bool {
    omnitrade_exchange::okx::instrument::is_contract_id(symbol)
}

/// InstType für ein kontraktbasiertes Symbol: XPERP und datierte Futures ->
/// Futures, SWAP -> Swap. Unbekanntes bleibt Swap (bisheriges Verhalten).
fn inst_type_for_symbol(symbol: &str) -> InstType {
    omnitrade_exchange::okx::instrument::inst_type_for_symbol(symbol).unwrap_or(InstType::Swap)
}

/// Hebel aus `OKX_LEVERAGE` (Default 1, Deckel [`swap::MAX_LEVERAGE`]).
fn okx_leverage() -> Result<u32, Box<dyn std::error::Error>> {
    let raw = std::env::var("OKX_LEVERAGE").unwrap_or_else(|_| "1".to_string());
    parse_leverage(&raw).map_err(Into::into)
}

/// Reine Validierung des Hebels, getrennt von der Umgebungsvariable für Tests.
fn parse_leverage(raw: &str) -> Result<u32, String> {
    let lever: u32 = raw
        .trim()
        .parse()
        .map_err(|_| format!("OKX_LEVERAGE muss eine ganze Zahl sein, war '{raw}'"))?;
    if lever == 0 || lever > swap::MAX_LEVERAGE {
        return Err(format!(
            "OKX_LEVERAGE muss 1..={} sein, war {lever}",
            swap::MAX_LEVERAGE
        ));
    }
    Ok(lever)
}

/// Konto und Instrument für SWAP vorbereiten, fail closed: one-way-Modus prüfen,
/// Handelsregeln laden, Hebel (isolated) setzen. Jeder Fehler stoppt den Start.
async fn prepare_okx_swap(
    client: &OkxRestClient,
    symbol: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let symbol = symbol.to_uppercase();
    let lever = okx_leverage()?;
    client.ensure_net_mode().await?;
    let inst_type = inst_type_for_symbol(&symbol);
    let inst = client.load_instrument(&symbol, inst_type).await?;
    info!(
        "{} {symbol}: ctVal {} {}, lotSz {}, minSz {} Kontrakte, Abrechnung {}",
        inst_type.as_api_str(),
        inst.ct_val,
        symbol.split('-').next().unwrap_or(""),
        inst.lot_sz,
        inst.min_sz,
        inst.settle_ccy.as_deref().unwrap_or("(nicht angegeben)")
    );
    // Die Abrechnungswaehrung des KONTOS schlaegt die des Instruments: bei XPERP
    // sagt das Instrument `USD`, das Konto rechnet in USDC ab. Ohne diese
    // Reihenfolge faellt die Equity auf den uPnL zusammen (Drawdown in Millionen).
    let konto_settle = match client.account_settle_ccy().await {
        Ok(ccy) => ccy,
        Err(e) => {
            warn!("Kontokonfiguration nicht lesbar ({e}) - nehme die des Instruments");
            None
        }
    };
    let settle = konto_settle.or(inst.settle_ccy);
    info!(
        "{symbol}: Equity-Bewertung in {}",
        settle.as_deref().unwrap_or("(Quote des Symbols)")
    );
    client.set_leverage(&symbol, lever).await?;
    info!("{symbol}: isolated, one-way, Hebel {lever}x");
    Ok(settle)
}

/// OKX-Zugänge: `OKX_ENV_ONLY=1` erzwingt die zusammengehörenden
/// `OKX_API_KEY`/`OKX_API_SECRET`/`OKX_PASSPHRASE`; sonst hat der Keystore Vorrang.
fn load_okx_credentials(
    network: Network,
) -> Result<(credentials::ApiCredentials, String), Box<dyn std::error::Error>> {
    let env_only = std::env::var("OKX_ENV_ONLY").is_ok_and(|v| v == "1");
    let creds = if env_only {
        let key =
            std::env::var("OKX_API_KEY").map_err(|_| "OKX_ENV_ONLY=1 requires OKX_API_KEY")?;
        let secret = std::env::var("OKX_API_SECRET")
            .map_err(|_| "OKX_ENV_ONLY=1 requires OKX_API_SECRET")?;
        credentials::ApiCredentials::new(key, secret, network)
    } else if Keystore::credentials_exist("okx", network) {
        Keystore::load_credentials("okx", network).map_err(|e| format!("Keystore: {e}"))?
    } else {
        let key = std::env::var("OKX_API_KEY").map_err(|_| {
            "Keine OKX-Zugänge: weder Keystore-Eintrag omnitrade/okx/<netzwerk> noch OKX_API_KEY"
        })?;
        let secret = std::env::var("OKX_API_SECRET")
            .map_err(|_| "OKX_API_SECRET environment variable not set")?;
        credentials::ApiCredentials::new(key, secret, network)
    };
    let passphrase = std::env::var("OKX_PASSPHRASE").map_err(|_| {
        if env_only {
            "OKX_ENV_ONLY=1 requires OKX_PASSPHRASE"
        } else {
            "OKX_PASSPHRASE environment variable not set"
        }
    })?;
    Ok((creds, passphrase))
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
