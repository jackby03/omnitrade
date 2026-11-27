# CORE-005: Order Domain Types

## Status
✅ DONE

## Description
Defined shared order domain types used across `omnitrade-engine`, `omnitrade-script`, and `omnitrade-exchange`:
- `OrderSide` enum (Buy/Sell)
- `OrderType` enum (Market/Limit/StopLoss/TakeProfit)
- `OrderStatus` enum (New/PartiallyFilled/Filled/Canceled/PendingCancel/Rejected/Expired)
- `Order` struct with complete lifecycle tracking

## Implementation Details
- **File**: `omnitrade-core/src/order.rs` (174 lines)
- **Exports**: All types re-exported in `omnitrade-core/src/lib.rs`
- **Serialization**: Full serde support with derive macros
- **Display**: Implemented for all enums per Binance API naming conventions
- **Tests**: Unit tests inline with enum/struct Display tests and serialization roundtrip

## Dependencies Added
- `serde`, `serde_json` to `omnitrade-core/Cargo.toml`

## Next Phase
Ready for EXCH-006 (ExchangeTrading trait) which depends on Order types.
