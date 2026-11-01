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

/// OKX-Zugänge: Keystore (`omnitrade/okx/<netzwerk>`) hat Vorrang, sonst
/// `OKX_API_KEY`/`OKX_API_SECRET`. Die API-Passphrase kommt immer aus `OKX_PASSPHRASE`.
fn load_okx_credentials(
    network: Network,
) -> Result<(credentials::ApiCredentials, String), Box<dyn std::error::Error>> {
    let creds = if Keystore::credentials_exist("okx", network) {
        Keystore::load_credentials("okx", network).map_err(|e| format!("Keystore: {e}"))?
    } else {
        let key = std::env::var("OKX_API_KEY").map_err(|_| {
            "Keine OKX-Zugänge: weder Keystore-Eintrag omnitrade/okx/<netzwerk> noch OKX_API_KEY"
        })?;
        let secret = std::env::var("OKX_API_SECRET")
            .map_err(|_| "OKX_API_SECRET environment variable not set")?;
        credentials::ApiCredentials::new(key, secret, network)
    };
    let passphrase = std::env::var("OKX_PASSPHRASE")
        .map_err(|_| "OKX_PASSPHRASE environment variable not set")?;
    Ok((creds, passphrase))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn run(
        script: Option<&str>,
        grid: Option<&str>,
        symbol: &str,
        exchange: &str,
    ) -> Result<(), String> {
        handle_live(&LiveArgs {
            script_path: script.map(Path::new),
            grid_config_path: grid.map(Path::new),
            symbol,
            exchange,
            network: "testnet",
            initial_capital: 1000.0,
            order_quantity: 0.001,
            max_notional_usd: None,
            confirm_real_money: false,
            allow_shorts: false,
            status_file: None,
            interval: "1m",
        })
        .await
        .map_err(|e| e.to_string())
    }

    #[tokio::test]
    async fn kraken_trading_is_refused_with_reason() {
        // Act
        let err = run(Some("s.omni"), None, "BTC/USD", "kraken")
            .await
            .expect_err("refused");

        // Assert
        assert!(
            err.contains("Kraken") && err.contains("kein Testnet"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn grid_is_binance_only() {
        // Act
        let err = run(None, Some("g.json"), "BTC-USDT", "okx")
            .await
            .expect_err("refused");

        // Assert
        assert!(err.contains("nur auf Binance"), "{err}");
    }

    #[tokio::test]
    async fn unknown_exchange_is_rejected() {
        // Act
        let err = run(Some("s.omni"), None, "BTCUSDT", "bybit")
            .await
            .expect_err("rejected");

        // Assert
        assert!(err.contains("Unbekannte Börse"), "{err}");
    }

    #[tokio::test]
    async fn okx_without_credentials_fails_before_any_connection() {
        // Arrange: Keystore/Umgebung dürfen keine OKX-Zugänge liefern.
        if Keystore::credentials_exist("okx", Network::Testnet)
            || std::env::var("OKX_API_KEY").is_ok()
        {
            return; // Entwicklerrechner mit echten Zugängen: Test nicht aussagekräftig.
        }
        let script = std::env::temp_dir().join("omnitrade-live-okx-test.omni");
        std::fs::write(
            &script, "buy()
",
        )
        .expect("write script");

        // Act
        let err = run(script.to_str(), None, "BTC-USDT", "okx")
            .await
            .expect_err("no creds");
        let _ = std::fs::remove_file(&script);

        // Assert
        assert!(err.contains("OKX"), "{err}");
    }

    fn mainnet(
        exchange: &'static str,
        grid: Option<&'static str>,
        cap: Option<f64>,
        confirm: bool,
    ) -> LiveArgs<'static> {
        LiveArgs {
            script_path: if grid.is_some() {
                None
            } else {
                Some(Path::new("s.omni"))
            },
            grid_config_path: grid.map(Path::new),
            symbol: "BTC-USDC",
            exchange,
            network: "mainnet",
            initial_capital: 1000.0,
            order_quantity: 0.0001,
            max_notional_usd: cap,
            confirm_real_money: confirm,
            allow_shorts: false,
            status_file: None,
            interval: "1m",
        }
    }

    async fn rejected(args: LiveArgs<'static>) -> String {
        handle_live(&args)
            .await
            .expect_err("must be rejected")
            .to_string()
    }

    #[tokio::test]
    async fn mainnet_requires_explicit_real_money_confirmation() {
        let err = rejected(mainnet("okx", None, Some(20.0), false)).await;
        assert!(err.contains("--confirm-real-money"), "{err}");
    }

    #[test]
    fn leverage_is_bounded_to_one_through_ten() {
        assert_eq!(parse_leverage("1"), Ok(1));
        assert_eq!(parse_leverage(" 10 "), Ok(10));
        assert!(parse_leverage("0").is_err());
        assert!(parse_leverage("11").is_err());
        assert!(parse_leverage("2.5").is_err());
        assert!(parse_leverage("").is_err());
    }

    #[test]
    fn swap_symbols_are_detected_case_insensitively() {
        assert!(is_contract_symbol("BTC-USDT-SWAP"));
        assert!(is_contract_symbol("btc-usdt-swap"));
        assert!(is_contract_symbol("BTC-USD_UM_XPERP-310404"));
        assert!(is_contract_symbol("btc-usd_um_xperp-310404"));
        // Datierte Futures (einziger auf dem EEA-Demokonto handelbarer Typ)
        assert!(is_contract_symbol("BTC-USD_UM-261030"));
        assert!(is_contract_symbol("btc-usd_um-261030"));
        assert!(is_contract_symbol("BTC-USD-261030"));
        assert!(!is_contract_symbol("BTC-USDT"));
        assert_eq!(
            inst_type_for_symbol("BTC-USD_UM_XPERP-310404"),
            InstType::Futures
        );
        assert_eq!(inst_type_for_symbol("BTC-USD_UM-261030"), InstType::Futures);
        assert_eq!(inst_type_for_symbol("BTC-USD-261030"), InstType::Futures);
        assert_eq!(inst_type_for_symbol("BTC-USDT-SWAP"), InstType::Swap);
    }

    #[tokio::test]
    async fn mainnet_is_okx_only() {
        for exchange in ["binance", "kraken"] {
            let err = rejected(mainnet(exchange, None, Some(20.0), true)).await;
            assert!(
                err.contains("nur für OKX") || err.contains("Kraken"),
                "{exchange}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn mainnet_grid_stays_blocked() {
        let err = rejected(mainnet("okx", Some("g.json"), Some(20.0), true)).await;
        assert!(err.contains("Grid-Live"), "{err}");
    }

    #[tokio::test]
    async fn mainnet_needs_a_cap_within_the_hard_limit() {
        for cap in [
            None,
            Some(0.0),
            Some(-5.0),
            Some(MAINNET_HARD_CAP + 1.0),
            Some(f64::NAN),
        ] {
            let err = rejected(mainnet("okx", None, cap, true)).await;
            assert!(err.contains("--max-notional-usd"), "{cap:?}: {err}");
        }
    }

    #[tokio::test]
    async fn mainnet_still_needs_the_environment_approval() {
        if std::env::var("OMNITRADE_MAINNET_APPROVED").is_ok() {
            return; // Rechner mit gesetzter Freigabe: Test nicht aussagekräftig.
        }
        let err = rejected(mainnet("okx", None, Some(20.0), true)).await;
        assert!(err.contains("OMNITRADE_MAINNET_APPROVED"), "{err}");
    }

    #[tokio::test]
    async fn mainnet_perp_stays_blocked_even_with_all_approvals() {
        // Die Sperre greift vor den Umgebungs-Checks und ist daher unabhaengig
        // von OMNITRADE_MAINNET_APPROVED und OMNITRADE_PERP_MAINNET_APPROVED.
        let mut args = mainnet("okx", None, Some(20.0), true);
        args.symbol = "SOL-USD_UM_XPERP-310404";
        let err = rejected(args).await;
        assert!(err.contains("gesperrt"), "{err}");
    }

    #[tokio::test]
    async fn non_positive_testnet_cap_is_rejected() {
        let mut args = mainnet("okx", None, Some(-1.0), false);
        args.network = "testnet";
        let err = rejected(args).await;
        assert!(
            err.contains("--max-notional-usd muss positiv sein"),
            "{err}"
        );
    }
}
