//! Exchange selection shared by CLI and GUI.

use std::fmt;
use std::str::FromStr;

use omnitrade_core::Candle;

use crate::credentials::Network;
use crate::error::ExchangeError;
use crate::traits::ExchangeStream;
use crate::{binance, kraken, okx};
use crate::{BinanceClient, KrakenClient, OkxClient};

/// Supported exchanges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exchange {
    /// Binance Spot (testnet and mainnet).
    Binance,
    /// Kraken Spot (market data only; no testnet, trading is refused).
    Kraken,
    /// OKX Spot (`Network::Testnet` = demo trading).
    Okx,
}

impl Exchange {
    /// All exchanges, for help texts and tests.
    pub const ALL: [Exchange; 3] = [Exchange::Binance, Exchange::Kraken, Exchange::Okx];

    /// Lowercase identifier used on the command line and as keystore name.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Binance => "binance",
            Self::Kraken => "kraken",
            Self::Okx => "okx",
        }
    }

    /// A sensible default symbol in the exchange's own notation.
    #[must_use]
    pub fn default_symbol(self) -> &'static str {
        match self {
            Self::Binance => "BTCUSDT",
            Self::Kraken => "BTC/USD",
            Self::Okx => "BTC-USDT",
        }
    }

    /// `true` if the exchange offers a test environment for orders.
    #[must_use]
    pub fn has_test_environment(self) -> bool {
        !matches!(self, Self::Kraken)
    }

    /// Creates a market-data stream client for the network.
    #[must_use]
    pub fn stream(self, network: Network) -> Box<dyn ExchangeStream> {
        match self {
            Self::Binance => Box::new(BinanceClient::for_network(network)),
            Self::Kraken => Box::new(KrakenClient::new()),
            Self::Okx => Box::new(OkxClient::for_network(network)),
        }
    }

    /// Loads recent closed candles (oldest first) over the public REST API.
    ///
    /// # Errors
    /// Fetch or parse errors of the respective exchange.
    pub async fn fetch_history(
        self,
        network: Network,
        symbol: &str,
        interval: &str,
        limit: usize,
    ) -> Result<Vec<Candle>, ExchangeError> {
        match self {
            Self::Binance => binance::fetch_klines(network, symbol, interval, limit).await,
            Self::Kraken => kraken::klines::fetch_klines(symbol, interval, limit).await,
            Self::Okx => okx::klines::fetch_klines(network, symbol, interval, limit).await,
        }
    }
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for Exchange {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "binance" => Ok(Self::Binance),
            "kraken" => Ok(Self::Kraken),
            "okx" => Ok(Self::Okx),
            other => Err(format!("Unbekannte Börse '{other}' (binance|kraken|okx)")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_insensitively_and_roundtrips() {
        for exchange in Exchange::ALL {
            assert_eq!(exchange.id().parse::<Exchange>(), Ok(exchange));
            assert_eq!(exchange.to_string(), exchange.id());
        }
        assert_eq!("OKX".parse::<Exchange>(), Ok(Exchange::Okx));
        assert!("bybit".parse::<Exchange>().is_err());
    }

    #[test]
    fn default_symbols_use_each_exchanges_notation() {
        assert_eq!(Exchange::Binance.default_symbol(), "BTCUSDT");
        assert_eq!(Exchange::Kraken.default_symbol(), "BTC/USD");
        assert_eq!(Exchange::Okx.default_symbol(), "BTC-USDT");
    }

    #[test]
    fn only_kraken_lacks_a_test_environment() {
        assert!(Exchange::Binance.has_test_environment());
        assert!(Exchange::Okx.has_test_environment());
        assert!(!Exchange::Kraken.has_test_environment());
    }

    #[tokio::test]
    async fn stream_factory_accepts_each_exchanges_default_symbol() {
        for exchange in Exchange::ALL {
            let mut stream = exchange.stream(Network::Testnet);
            assert!(
                stream
                    .subscribe_candles(exchange.default_symbol(), "1m")
                    .await
                    .is_ok(),
                "{exchange} must accept its default symbol"
            );
        }
    }
}
