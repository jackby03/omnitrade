# EXCH-007: Credentials & Keystore Integration

## Status
✅ DONE

## Description
Implemented secure credential management with OS keyring integration (Windows Credential Manager, macOS Keychain, Linux Secret Service) and strict Testnet/Mainnet separation.

## Modules

### credentials.rs (116 lines)
- `Network` enum (Testnet/Mainnet) - explicit designation prevents accidents
- `ApiCredentials` struct with:
  - `new(key, secret, network)` constructor
  - `is_testnet()` / `is_mainnet()` checks
  - `validate()` method (non-empty key/secret)
  - Display impl (masks key for logging)

### keystore.rs (230 lines)
- `Keystore` singleton manager with static methods:
  - `save_credentials(exchange, credentials)` - store in system keyring
  - `load_credentials(exchange, network)` - retrieve from keyring
  - `delete_credentials(exchange, network)` - cleanup
  - `credentials_exist(exchange, network)` - check without retrieving
- Keyring entry format: `omnitrade/{exchange}/{network}` (e.g., `omnitrade/binance/testnet`)
- Network mismatch detection (e.g., attempting to load mainnet as testnet fails)
- Full serde JSON serialization/deserialization

## Error Handling
Added new ExchangeError variants:
- `KeyringError(String)` - keyring access failures
- `InvalidCredentials(String)` - empty key/secret, validation failures
- `NetworkMismatch(String)` - network/credential mismatch protection

## Tests
- Inline unit tests for entry_key format, credentials roundtrip, network mismatch protection
- All tests marked with comments about keyring availability in CI

## Dependencies Added
- `keyring = "2"` to workspace dependencies
- `serde_json` for credential serialization

## Security Notes
- Testnet and Mainnet credentials are strictly separated by keyring entry names
- No credentials are logged (Display impl masks key)
- Validation enforced before storage

## Next Phase
Ready for EXCH-008 (HMAC signing) and EXCH-009 (REST client that uses credentials).
