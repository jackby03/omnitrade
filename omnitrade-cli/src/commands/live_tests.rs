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
    if Keystore::credentials_exist("okx", Network::Testnet) || std::env::var("OKX_API_KEY").is_ok()
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
