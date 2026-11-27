//! Grid trading configuration and order generation.
//!
//! Supports automatic placement of buy/sell orders at regular price intervals
//! below and above a starting price.

use serde::{Deserialize, Serialize};

/// Configuration for grid-based trading strategy.
///
/// Generates evenly-spaced buy orders below `start_price` and sell orders above.
/// Each level is separated by `spacing_pct` (e.g., 2% means 2% apart).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GridConfig {
    /// Number of buy/sell levels in the grid (e.g., 10 levels).
    pub levels: usize,

    /// Spacing between levels as a percentage (e.g., 0.02 for 2%).
    /// Must be positive and typically small (0.01 to 0.05).
    pub spacing_pct: f64,

    /// Position size per level in base asset quantity.
    /// Total grid size = levels * size_per_level.
    pub size_per_level: f64,

    /// Entry/reference price for grid center.
    pub start_price: f64,

    /// Leverage multiplier (1.0 for spot, >1 for margin trading).
    pub leverage: f64,
}

impl GridConfig {
    /// Creates a new GridConfig with validation.
    ///
    /// # Errors
    /// Returns `Err` if any parameter is invalid (non-positive, NaN, etc.)
    pub fn new(
        levels: usize,
        spacing_pct: f64,
        size_per_level: f64,
        start_price: f64,
        leverage: f64,
    ) -> Result<Self, String> {
        if levels == 0 {
            return Err("levels must be greater than 0".to_string());
        }
        if spacing_pct <= 0.0 || spacing_pct.is_nan() {
            return Err("spacing_pct must be positive and non-NaN".to_string());
        }
        if size_per_level <= 0.0 || size_per_level.is_nan() {
            return Err("size_per_level must be positive and non-NaN".to_string());
        }
        if start_price <= 0.0 || start_price.is_nan() {
            return Err("start_price must be positive and non-NaN".to_string());
        }
        if leverage <= 0.0 || leverage.is_nan() {
            return Err("leverage must be positive and non-NaN".to_string());
        }

        Ok(Self {
            levels,
            spacing_pct,
            size_per_level,
            start_price,
            leverage,
        })
    }

    /// Generates buy orders below start_price.
    ///
    /// Returns a vector of `(price, quantity)` tuples.
    /// Prices decrease geometrically by `spacing_pct` each level.
    pub fn generate_buy_orders(&self) -> Vec<(f64, f64)> {
        let mut orders = Vec::new();
        let multiplier = 1.0 - self.spacing_pct;

        for level in 1..=self.levels {
            let price = self.start_price * multiplier.powi(level as i32);
            let quantity = self.size_per_level * self.leverage;
            orders.push((price, quantity));
        }

        orders
    }

    /// Generates sell orders above start_price.
    ///
    /// Returns a vector of `(price, quantity)` tuples.
    /// Prices increase geometrically by `spacing_pct` each level.
    pub fn generate_sell_orders(&self) -> Vec<(f64, f64)> {
        let mut orders = Vec::new();
        let multiplier = 1.0 + self.spacing_pct;

        for level in 1..=self.levels {
            let price = self.start_price * multiplier.powi(level as i32);
            let quantity = self.size_per_level * self.leverage;
            orders.push((price, quantity));
        }

        orders
    }

    /// Loads GridConfig from a JSON string.
    ///
    /// # Errors
    /// Returns `Err` if JSON parsing fails or validation fails.
    pub fn from_json(json: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let raw: GridConfig = serde_json::from_str(json)?;
        // Deserialisierung umgeht `new`; Werte hier erneut prüfen.
        Ok(Self::new(
            raw.levels,
            raw.spacing_pct,
            raw.size_per_level,
            raw.start_price,
            raw.leverage,
        )?)
    }

    /// Serializes GridConfig to a JSON string.
    ///
    /// # Errors
    /// Returns `Err` if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_config_creation() {
        let result = GridConfig::new(10, 0.02, 1.0, 50_000.0, 1.0);
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.levels, 10);
        assert_eq!(config.spacing_pct, 0.02);
        assert_eq!(config.size_per_level, 1.0);
        assert_eq!(config.start_price, 50_000.0);
        assert_eq!(config.leverage, 1.0);
    }

    #[test]
    fn test_grid_config_validation_zero_levels() {
        let result = GridConfig::new(0, 0.02, 1.0, 50_000.0, 1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_grid_config_validation_negative_spacing() {
        let result = GridConfig::new(10, -0.02, 1.0, 50_000.0, 1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_grid_config_validation_nan_values() {
        let result = GridConfig::new(10, f64::NAN, 1.0, 50_000.0, 1.0);
        assert!(result.is_err());

        let result = GridConfig::new(10, 0.02, f64::NAN, 50_000.0, 1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_buy_orders() {
        let config = GridConfig::new(3, 0.1, 1.0, 100.0, 1.0).expect("config creation");
        let orders = config.generate_buy_orders();

        assert_eq!(orders.len(), 3);
        // Level 1: 100 * (1 - 0.1)^1 = 90
        assert!((orders[0].0 - 90.0).abs() < 1e-9);
        assert_eq!(orders[0].1, 1.0);

        // Level 2: 100 * (1 - 0.1)^2 = 81
        assert!((orders[1].0 - 81.0).abs() < 1e-9);
        assert_eq!(orders[1].1, 1.0);

        // Level 3: 100 * (1 - 0.1)^3 = 72.9
        assert!((orders[2].0 - 72.9).abs() < 1e-9);
        assert_eq!(orders[2].1, 1.0);
    }

    #[test]
    fn test_generate_sell_orders() {
        let config = GridConfig::new(3, 0.1, 1.0, 100.0, 1.0).expect("config creation");
        let orders = config.generate_sell_orders();

        assert_eq!(orders.len(), 3);
        // Level 1: 100 * (1 + 0.1)^1 = 110
        assert!((orders[0].0 - 110.0).abs() < 1e-9);
        assert_eq!(orders[0].1, 1.0);

        // Level 2: 100 * (1 + 0.1)^2 = 121
        assert!((orders[1].0 - 121.0).abs() < 1e-9);
        assert_eq!(orders[1].1, 1.0);

        // Level 3: 100 * (1 + 0.1)^3 = 133.1
        assert!((orders[2].0 - 133.1).abs() < 1e-9);
        assert_eq!(orders[2].1, 1.0);
    }

    #[test]
    fn test_generate_orders_with_leverage() {
        let config = GridConfig::new(2, 0.05, 1.0, 1000.0, 2.0).expect("config creation");
        let buy_orders = config.generate_buy_orders();
        let sell_orders = config.generate_sell_orders();

        // Quantity should be size_per_level * leverage
        assert_eq!(buy_orders[0].1, 2.0);
        assert_eq!(sell_orders[0].1, 2.0);
    }

    #[test]
    fn test_grid_config_json_roundtrip() {
        let original = GridConfig::new(5, 0.02, 0.5, 30_000.0, 1.0).expect("config creation");
        let json = original.to_json().expect("serialization");
        let deserialized = GridConfig::from_json(&json).expect("deserialization");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_grid_config_from_json_rejects_invalid_values() {
        let json = r#"{"levels": 0, "spacing_pct": 0.02, "size_per_level": 1.0,
            "start_price": 100.0, "leverage": 1.0}"#;
        assert!(GridConfig::from_json(json).is_err());
        let json = r#"{"levels": 3, "spacing_pct": -0.02, "size_per_level": 1.0,
            "start_price": 100.0, "leverage": 1.0}"#;
        assert!(GridConfig::from_json(json).is_err());
    }

    #[test]
    fn test_grid_config_json_parse() {
        let json = r#"{
            "levels": 10,
            "spacing_pct": 0.02,
            "size_per_level": 1.5,
            "start_price": 50000.0,
            "leverage": 1.0
        }"#;

        let config = GridConfig::from_json(json).expect("parsing");
        assert_eq!(config.levels, 10);
        assert_eq!(config.spacing_pct, 0.02);
        assert_eq!(config.size_per_level, 1.5);
        assert_eq!(config.start_price, 50_000.0);
        assert_eq!(config.leverage, 1.0);
    }
}
