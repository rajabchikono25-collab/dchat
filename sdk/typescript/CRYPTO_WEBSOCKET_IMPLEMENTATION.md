# dchat TypeScript SDK - Cryptography & WebSocket Implementation

This document describes the complete Ed25519 cryptography and WebSocket relay connection implementation for the dchat TypeScript/JavaScript SDK.

## Features Implemented

### 1. Ed25519 Cryptography (`src/crypto/keypair.ts`)

Complete implementation using `@noble/ed25519`:

- **Key Generation**: `generateKeyPair()` - Secure Ed25519 key pair generation
- **Message Signing**: `sign(message, privateKey)` - Ed25519 signature creation
- **Signature Verification**: `verify(message, signature, publicKey)` - Ed25519 signature verification

All cryptographic operations use the battle-tested `@noble/ed25519` library, which provides:
- Pure TypeScript implementation
- No native dependencies
- Constant-time operations
- NIST compliance

### 2. WebSocket Connection Manager (`src/crypto/websocket.ts`)

Production-ready WebSocket client for relay connections with:

- **Connection Management**:
  - Automatic connection with async/await API
  - Graceful disconnection
  - Connection state tracking

- **Reconnection Strategy**:
  - Configurable reconnect delay
  - Maximum reconnect attempts
  - Exponential backoff support (configurable)

- **Heartbeat/Ping**:
  - Automatic ping/pong heartbeat
  - Configurable ping interval
  - Connection health monitoring

- **Event System**:
  - Message handlers: `onMessage(handler)`
  - Error handlers: `onError(handler)`
  - Connect handlers: `onConnect(handler)`
  - Disconnect handlers: `onDisconnect(handler)`
  - Handler registration/unregistration

- **Error Handling**:
  - Graceful error propagation
  - Automatic reconnection on disconnect
  - Connection timeout handling

### 3. Client Integration (`src/client.ts`)

Complete integration of cryptography and networking:

- **Identity Management**:
  - Real Ed25519 key pair generation during client creation
  - Public key stored in identity
  - Private key securely managed

- **Network Connection**:
  - WebSocket connection to relay peers
  - Relay selection from bootstrap peers (round-robin for now)
  - Automatic reconnection on disconnect

- **Message Operations**:
  - `sendMessage(text)`: Sign and send messages via WebSocket
  - `receiveMessages()`: Receive messages from relay
  - Message signature generation with Ed25519

- **Cryptographic API**:
  - `signMessage(message)`: Sign arbitrary messages
  - `verifySignature(message, signature, publicKey)`: Verify signatures

## Dependencies Added

```json
{
  "dependencies": {
    "@noble/ed25519": "^2.0.0",  // Ed25519 cryptography
    "ws": "^8.14.0"               // WebSocket client (already present)
  },
  "devDependencies": {
    "@types/ws": "^8.5.0"         // TypeScript definitions for ws
  }
}
```

## Installation

```bash
cd sdk/typescript
npm install
npm run build
```

## Usage Examples

### Basic Client with Cryptography

```typescript
import { Client } from '@dchat/sdk';

// Create client (generates Ed25519 keys automatically)
const client = await Client.builder()
  .name('Alice')
  .bootstrapPeers(['ws://relay1.dchat.network:8080'])
  .build();

// Connect to network
await client.connect();

// Send signed message
await client.sendMessage('Hello, decentralized world!');

// Receive messages
const messages = await client.receiveMessages();

// Disconnect
await client.disconnect();
```

### Manual Cryptography

```typescript
import { generateKeyPair, sign, verify } from '@dchat/sdk';

// Generate key pair
const keyPair = await generateKeyPair();
console.log('Public key:', keyPair.publicKey);

// Sign a message
const message = 'Important message';
const signature = await sign(message, keyPair.privateKey);

// Verify signature
const isValid = await verify(message, signature, keyPair.publicKey);
console.log('Signature valid:', isValid);
```

### Direct WebSocket Usage

```typescript
import { WebSocketManager } from '@dchat/sdk';

const wsManager = new WebSocketManager({
  url: 'ws://relay.dchat.network:8080',
  reconnectDelay: 5000,
  maxReconnectAttempts: 10,
  pingInterval: 30000,
});

// Set up handlers
wsManager.onMessage((message) => {
  console.log('Received:', message);
});

wsManager.onConnect(() => {
  console.log('Connected to relay');
});

wsManager.onError((error) => {
  console.error('WebSocket error:', error);
});

// Connect
await wsManager.connect();

// Send message
await wsManager.send({ type: 'ping', data: 'hello' });

// Disconnect
await wsManager.disconnect();
```

## Architecture Notes

### Relay Selection

Currently uses simple round-robin selection from bootstrap peers. Production implementation should:
- Query relay reputation from blockchain
- Implement geographic proximity selection
- Support failover to multiple relays
- Implement relay quality scoring

### Message Encryption

The current implementation:
- Signs all messages with Ed25519
- Placeholder for end-to-end encryption with recipient public key
- TODO: Implement Noise Protocol handshake for E2EE

### WebSocket Protocol

Message format (JSON over WebSocket):
```json
{
  "type": "send_message",
  "message": {
    "id": "uuid",
    "senderId": "uuid",
    "content": { "type": "Text", "text": "message" },
    "timestamp": "ISO-8601",
    "signature": "hex-encoded-ed25519-sig",
    "publicKey": "hex-encoded-ed25519-pubkey"
  }
}
```

Relay should verify:
1. Signature matches public key
2. Sender ID is authenticated
3. Message timestamp is recent
4. Rate limits not exceeded

## Testing

```bash
npm test                 # Run all tests
npm run test:watch      # Watch mode
```

Unit tests provided for:
- Ed25519 key generation, signing, verification
- WebSocket connection lifecycle
- Client integration

Integration tests require a running dchat relay node.

## Production Readiness

✅ **Completed**:
- Ed25519 cryptography with @noble/ed25519
- WebSocket connection with reconnection
- Message signing and verification
- Event-driven message handling
- Client-relay integration

⚠️ **TODO**:
- End-to-end encryption (Noise Protocol)
- Relay reputation-based selection
- Message batching for efficiency
- Local message persistence
- Offline message queueing

## Security Considerations

1. **Private Key Storage**: Keys stored in memory only. For browser environments, consider:
   - IndexedDB for persistent storage
   - Web Crypto API for key derivation
   - Hardware security modules (HSM) for production

2. **Signature Verification**: All incoming messages MUST be signature-verified before processing

3. **Replay Protection**: TODO - implement nonce/timestamp verification

4. **Rate Limiting**: Client-side rate limiting not implemented - rely on relay enforcement

5. **Connection Security**: Use WSS (WebSocket Secure) in production, not WS

## References

- Ed25519: https://ed25519.cr.yp.to/
- @noble/ed25519: https://github.com/paulmillr/noble-ed25519
- WebSocket Protocol: https://datatracker.ietf.org/doc/html/rfc6455
- dchat Architecture: See `../../ARCHITECTURE-2.0.md`

