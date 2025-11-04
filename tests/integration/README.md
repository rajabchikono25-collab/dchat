# Integration Tests

Integration tests for dchat network infrastructure.

## Prerequisites

```bash
# Set config file
export DCHAT_CONFIG=../config.dev.toml

# For TURN tests, set credentials
export TURN_USERNAME=dchat
export TURN_SECRET=<from-deployment>
```

## Running Tests

```bash
# All integration tests
cargo test --test integration_* -- --nocapture

# Individual tests
cargo test --test integration_stun
cargo test --test integration_turn
cargo test --test integration_bootstrap
cargo test --test integration_circuit
```

## Test Coverage

- **STUN**: NAT detection, external IP discovery
- **TURN**: Relay allocation, credential authentication
- **Bootstrap**: Peer discovery, DHT bootstrapping
- **Circuit**: Full onion routing setup and teardown
