# EXCH-008: HMAC-SHA256 Signing

## Status
✅ DONE

## Description
Implemented Binance REST API HMAC-SHA256 query string signing per official Binance API documentation.

## Module
**File**: `omnitrade-exchange/src/binance/signing.rs` (170 lines)

## Core Implementation
- `BinanceSigner` struct with static methods:
  - `sign(query_string, secret) -> Result<String>` - raw HMAC signing (hex-encoded)
  - `build_signed_query(query_string, signature) -> String` - combine params + signature
  - `sign_request(query_string, secret, timestamp) -> Result<String>` - complete flow with timestamp injection

## Algorithm
1. Message = `query_string&timestamp={timestamp}`
2. Signature = hex(HMAC-SHA256(message, secret_key))
3. Final Query = `{message}&signature={signature}`

## Dependencies
- `hmac`, `sha2` from workspace dependencies
- `hex` for hex encoding of signature bytes

## Error Handling
- Returns `ExchangeError::InvalidCredentials` for invalid secret key format
- All errors typed as `Result<String, ExchangeError>`

## Tests
- 7 inline unit tests covering:
  - Empty/parameterized queries
  - Signature determinism (same input = same output)
  - Secret/query uniqueness (different inputs = different signatures)
  - Signature format validation (hex digits)
  - Complete request flow with timestamp

## Security Notes
- Uses HMAC (keyed hash) not simple hash - prevents tampering
- Hex encoding standard for Binance API
- Timestamp prevents replay attacks

## Next Phase
Ready for EXCH-009 (REST client uses BinanceSigner for all authenticated requests).
