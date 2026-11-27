# Phase 5 Migration Guide: Long + Short Position Support

## Overview
**BREAKING CHANGE**: Phase 5 introduces simultaneous long and short positions on the same symbol. The `AccountState::positions` HashMap now uses `(String, Side)` tuples as keys instead of just `String`.

This enables margin/futures trading with hedging (e.g., long 1 BTC and short 2 BTC on BTCUSDT simultaneously).

## Key Changes

### 1. Position Lookup API
**Before:**
```rust
let pos = account.get_position("BTCUSDT")?;  // Returns single position
```

**After:**
```rust
let long_pos = account.get_position("BTCUSDT", Side::Buy)?;  // Get long position
let short_pos = account.get_position("BTCUSDT", Side::Sell)?; // Get short position
let all = account.get_all_positions_for_symbol("BTCUSDT");     // Get both
```

### 2. Position Updates
**Before:**
```rust
account.update_unrealized_pnl("BTCUSDT", 55_000.0)?;
```

**After:**
```rust
account.update_unrealized_pnl("BTCUSDT", Side::Buy, 55_000.0)?;  // Specific side
```

### 3. Position Closing
**New Method:**
```rust
account.close_position("BTCUSDT", Side::Buy)?;  // Explicitly close a position
```

### 4. Fill Application Semantics
- **Buy orders** always create/add to long positions (symbol, Side::Buy)
- **Sell orders** always create/add to short positions (symbol, Side::Sell)
- **No automatic position closing** - opposite-side fills now create separate positions

**Example:**
```rust
let account = AccountState::new(100_000.0)?;

// Fill 1: Buy 1 BTC at 50,000
account.apply_fill(&buy_fill)?;
// Creates: (BTCUSDT, Buy) -> 1 BTC long position

// Fill 2: Sell 2 BTC at 51,000
account.apply_fill(&sell_fill)?;
// Creates: (BTCUSDT, Sell) -> 2 BTC short position
// Note: DOES NOT close the long position!
```

## Migration Checklist

### For Direct Position Access
- [ ] Update all `get_position(symbol)` calls to `get_position(symbol, side)`
- [ ] Update all `update_unrealized_pnl(symbol, price)` to `update_unrealized_pnl(symbol, side, price)`
- [ ] Use `get_all_positions_for_symbol(symbol)` when iterating all positions for a symbol

### For Position Iteration
**Before:**
```rust
for (symbol, pos) in &account.positions {
    println!("{}: {}", symbol, pos.quantity);
}
```

**After:**
```rust
for ((symbol, side), pos) in &account.positions {
    println!("{} {:?}: {}", symbol, side, pos.quantity);
}
```

### For Backtest Logic
If your strategy relied on automatic position closing (buy to close shorts, etc.):
- Modify strategy logic to explicitly handle long/short relationships
- Use `account.close_position(symbol, side)` when implementing manual position closure
- Or refactor to natural long-only or short-only strategies

### For Account Snapshots
If serializing AccountState:
- The `positions` HashMap key is now `(String, Side)` - adjust serialization code
- Ensure compatibility with any persistence layers

## Backward Compatibility Helpers
**None provided in Phase 5** - this is a breaking change. All code must migrate.

### Suggested Migration Pattern
1. Update position lookup calls first (compile errors guide this)
2. Update unrealized PnL calculation loops
3. Test with Phase 5 test suite
4. Verify backtests with hedging scenarios

## Testing Phase 5 Features
```rust
#[test]
fn test_hedge_position() {
    let mut acc = AccountState::new(100_000.0)?;
    
    // Long 1 BTC
    let buy = OrderFill::new(..., Side::Buy, 50_000.0, 1.0, ...);
    acc.apply_fill(&buy)?;
    
    // Short 1.5 BTC (hedge)
    let sell = OrderFill::new(..., Side::Sell, 51_000.0, 1.5, ...);
    acc.apply_fill(&sell)?;
    
    // Both positions coexist
    assert!(acc.get_position("BTCUSDT", Side::Buy).is_some());
    assert!(acc.get_position("BTCUSDT", Side::Sell).is_some());
    
    // Mark-to-market both
    acc.update_unrealized_pnl("BTCUSDT", Side::Buy, 52_000.0)?;
    acc.update_unrealized_pnl("BTCUSDT", Side::Sell, 52_000.0)?;
}
```

## Error Handling
Errors remain the same:
- `InsufficientBalance` - when cash is inadequate for a fill
- `SymbolNotFound` - when trying to close/update a position that doesn't exist
  - Now checks both symbol AND side

## Performance Notes
- HashMap key changes from `String` to `(String, Side)`
- Lookups remain O(1), slightly larger key overhead
- Iteration now includes both long and short for a symbol - filter as needed

## Questions?
Refer to the test suite in `omnitrade-engine/src/account.rs` for working examples.
