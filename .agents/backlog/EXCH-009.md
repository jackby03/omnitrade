# EXCH-009: Binance REST API Client

## Status
✅ DONE

## Description
Implemented full Binance Spot REST API client implementing `ExchangeTrading` trait.
Supports both Testnet and Mainnet with automatic URL selection based on Network enum.

## Module
**File**: `omnitrade-exchange/src/binance/rest.rs` (340 lines)

## Core Implementation

### BinanceRestClient struct
- Holds: `http_client: Client`, `base_url: String`, `credentials: ApiCredentials`
- Base URLs:
  - Testnet: `https://testnet.binance.vision`
  - Mainnet: `https://api.binance.com`
- Network enforced at creation: wrong network rejects client construction

### ExchangeTrading Implementation
- `place_order(req)` → POST `/api/v3/order` → Order response parsing
- `cancel_order(order_id, symbol)` → DELETE `/api/v3/order`
- `get_open_orders()` → GET `/api/v3/openOrders`
- `get_balance()` → GET `/api/v3/account` → HashMap<asset, AccountBalance>
- `get_order(order_id, symbol)` → default impl via openOrders

### Internal Methods
- `signed_request(method, endpoint, params)` - applies HMAC signing + timestamp
- `parse_order_response()` - converts Binance JSON to Order domain type

## Request/Response Types
- **Requests**: OrderRequest (converted from ExchangeTrading API)
- **Responses**:
  - `BinanceOrderResponse` - raw Binance order JSON
  - `BinanceAccountResponse` - account data with balances
  - `BinanceBalance` - individual asset balance

## Error Handling
- Connection failures → `ExchangeError::ConnectionFailed`
- JSON parse failures → `ExchangeError::MessageParseFailed`
- Invalid order params → `ExchangeError::InvalidRequest`
- API errors → `ExchangeError::ApiError`
- HTTP non-2xx → extract error from response body

## Dependencies
- `reqwest` (async HTTP client)
- `serde`, `serde_json` (JSON (de)serialization)
- `urlencoding` (URL parameter encoding)
- `async-trait` (async trait definition)

## Tests
- Client creation: testnet vs mainnet base URL selection
- Invalid credentials rejection
- Placeholder credential acceptance (for testing without real keys)

## CRITICAL: Testnet-Only Phase 1
- Network::Testnet hardcoded in tests
- Mainnet URLs present but untested in Phase 1
- Risk gate: All trades MUST validate network == Testnet before API call (TODO: add at OrderRequest level)

## Next Phase
Ready for EXCH-010 (integration tests). Phase 2 will add risk management layer before Mainnet support.
