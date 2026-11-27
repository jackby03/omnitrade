# EXCH-006: ExchangeTrading Trait Definition

## Status
✅ DONE

## Description
Defined the core trading interface trait `ExchangeTrading` as pure abstraction (no implementation).

## API Specification
```rust
pub trait ExchangeTrading: Send + Sync {
    async fn place_order(&self, req: &OrderRequest) -> Result<Order, ExchangeError>;
    async fn cancel_order(&self, order_id: &str, symbol: &str) -> Result<(), ExchangeError>;
    async fn get_open_orders(&self) -> Result<Vec<Order>, ExchangeError>;
    async fn get_balance(&self) -> Result<Account, ExchangeError>;
    async fn get_order(&self, order_id: &str, _symbol: &str) -> Result<Option<Order>, ExchangeError>; // optional default impl
}
```

## Supporting Types
- `OrderRequest` struct with factory methods:
  - `OrderRequest::market(symbol, side, quantity)`
  - `OrderRequest::limit(symbol, side, quantity, price)`
- `Account` and `AccountBalance` structs for balance tracking

## Implementation Details
- **File**: `omnitrade-exchange/src/trading.rs` (165 lines)
- **Attributes**: async_trait for async trait definition
- **Tests**: Unit tests for OrderRequest creation, display, serialization
- **Error Handling**: All methods return `ExchangeError` on failure

## Dependencies
- `async-trait` (already in Cargo.toml)
- `serde`, `serde_json`

## Next Phase
Ready for EXCH-007 (credentials/keystore) and EXCH-009 (REST implementation).
