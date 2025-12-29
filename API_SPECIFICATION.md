# dchat API Specification

**Version**: 1.0  
**Last Updated**: October 28, 2025  
**Status**: Production-Ready (Core APIs), Beta (Advanced APIs)

---

## Table of Contents

1. [Overview](#overview)
2. [HTTP API](#http-api)
3. [P2P/libp2p API](#p2plibp2p-api)
4. [gRPC API](#grpc-api)
5. [WebSocket API](#websocket-api)
6. [Error Handling](#error-handling)
7. [Rate Limiting](#rate-limiting)
8. [Authentication](#authentication)
9. [Versioning](#versioning)

---

## Overview

dchat exposes multiple API surfaces for different use cases:

| API Type      | Protocol         | Purpose                   | Stability |
| ------------- | ---------------- | ------------------------- | --------- |
| **HTTP**      | REST             | Health, metrics, status   | ✅ Stable |
| **P2P**       | libp2p protocols | Peer communication        | ✅ Stable |
| **gRPC**      | Protocol Buffers | High-performance clients  | ✅ Stable |
| **WebSocket** | WS/WSS           | Real-time browser clients | ⚠️ Beta   |

---

## HTTP API

### Base URL

```
http://localhost:8080/api/v1
```

### Health & Status Endpoints

#### `GET /health`

**Purpose**: Liveness probe (is the service running?)

**Response (200 OK)**:

```json
{
  "status": "ok",
  "version": "1.0.0",
  "timestamp": 1698518400,
  "uptime_seconds": 86400
}
```

**Codes**:

- `200 OK` - Service is alive
- `503 Service Unavailable` - Service degraded

---

#### `GET /ready`

**Purpose**: Readiness probe (is it ready to serve traffic?)

**Response (200 OK)**:

```json
{
  "ready": true,
  "connected_peers": 45,
  "synced_height": 1000000,
  "message_queue_depth": 150
}
```

**Codes**:

- `200 OK` - Ready to handle requests
- `503 Service Unavailable` - Not ready (syncing, low peer count, etc.)

---

#### `GET /status`

**Purpose**: Detailed node status

**Response (200 OK)**:

```json
{
  "node_type": "relay",
  "peer_id": "12D3KooWABC...",
  "version": "1.0.0",
  "network": {
    "connected_peers": 45,
    "pending_connections": 3,
    "bandwidth_in_mbps": 12.5,
    "bandwidth_out_mbps": 8.3
  },
  "relay": {
    "messages_forwarded": 1000000,
    "delivery_success_rate": 0.995,
    "average_delivery_time_ms": 250,
    "queue_size": 150,
    "uptime_percentage": 99.9
  },
  "storage": {
    "messages_stored": 500000,
    "database_size_mb": 2048,
    "backup_last_timestamp": 1698518400
  }
}
```

---

### Message API

#### `POST /messages`

**Purpose**: Send a message

**Request**:

```json
{
  "recipient_id": "12D3KooWXYZ...",
  "content": "Hello, world!",
  "channel_id": "general",
  "timestamp_ms": 1698518400000,
  "encryption_scheme": "noise_xx"
}
```

**Response (200 OK)**:

```json
{
  "message_id": "msg-abc123def456",
  "status": "forwarded",
  "relay_count": 3,
  "delivery_time_ms": 245
}
```

**Error Responses**:

- `400 Bad Request` - Invalid message format
- `401 Unauthorized` - Authentication failed
- `429 Too Many Requests` - Rate limit exceeded
- `503 Service Unavailable` - Relay overloaded

---

### Secure Message Envelope (QGE v1)

**Version**: 1.0 (Quorum-Gated Encryption)  
**Status**: Specification (Implementation Q1 2026)

The secure message envelope provides end-to-end encryption with quorum-gated read access. Messages are encrypted under per-message keys derived from Double Ratchet (1:1) or group protocol (channels), and the message keys are wrapped under a Storage Unlock Key (SUK) that requires periodic quorum authorization to unlock.

#### Wire Format

```
┌─────────────────────────────────────────────────────────────────┐
│                    SECURE MESSAGE ENVELOPE v1                   │
├─────────────────────────────────────────────────────────────────┤
│ version          : u8          (0x01 for QGE v1)                │
│ envelope_type    : u8          (0x01=1:1, 0x02=channel)         │
│ flags            : u16         (see flag definitions)           │
├─────────────────────────────────────────────────────────────────┤
│ message_id       : [u8; 32]    (unique message identifier)      │
│ conversation_id  : [u8; 32]    (channel_id or 1:1 conv hash)    │
│ sender_device_id : [u8; 32]    (sender's device identifier)     │
│ timestamp_ms     : u64         (send timestamp)                 │
├─────────────────────────────────────────────────────────────────┤
│ RATCHET HEADER (for Double Ratchet / Sender Keys)               │
│ sender_ratchet_key : [u8; 32]  (sender's current DH public key) │
│ previous_chain_len : u32       (# of messages in prev chain)    │
│ message_index      : u32       (index in current chain)         │
├─────────────────────────────────────────────────────────────────┤
│ CIPHERTEXT                                                      │
│ ciphertext_len   : u32         (length of encrypted payload)    │
│ ciphertext       : [u8; *]     (AEAD encrypted message content) │
├─────────────────────────────────────────────────────────────────┤
│ KEY WRAPPING (for at-rest storage)                              │
│ wrapped_key_len  : u16         (length of wrapped key blob)     │
│ wrapped_key      : [u8; *]     (message key wrapped under SUK)  │
│ unlock_epoch     : u64         (epoch when message was stored)  │
├─────────────────────────────────────────────────────────────────┤
│ AUTHENTICATION                                                  │
│ sender_signature : [u8; 64]    (Ed25519 signature over envelope)│
└─────────────────────────────────────────────────────────────────┘
```

#### Flag Definitions

| Bit  | Name                  | Description                                |
| ---- | --------------------- | ------------------------------------------ |
| 0    | `EPHEMERAL`           | Message should be deleted after read       |
| 1    | `HIGH_SECURITY`       | Requires user presence to decrypt          |
| 2    | `FORWARD_LOCKED`      | Cannot be forwarded to other conversations |
| 3    | `EMERGENCY_REVOCABLE` | Honor emergency lock immediately           |
| 4-7  | Reserved              | Must be zero                               |
| 8-15 | `TTL_HOURS`           | Time-to-live in hours (0 = no expiry)      |

#### JSON Representation (API)

**Sending a Secure Message**:

```json
POST /api/v2/messages/secure

{
  "version": 1,
  "envelope_type": "direct",
  "recipient_id": "12D3KooWXYZ...",
  "conversation_id": "conv-abc123...",
  "sender_device_id": "dev-xyz789...",

  "ratchet_header": {
    "sender_ratchet_key": "7d8f9b6c1234567890abcdef...",
    "previous_chain_length": 5,
    "message_index": 42
  },

  "ciphertext": "base64_encoded_aead_ciphertext...",

  "key_wrapping": {
    "wrapped_key": "base64_encoded_wrapped_message_key...",
    "unlock_epoch": 1735500000
  },

  "flags": {
    "ephemeral": false,
    "high_security": false,
    "forward_locked": true,
    "ttl_hours": 0
  },

  "sender_signature": "base64_ed25519_signature..."
}
```

**Response (201 Created)**:

```json
{
  "message_id": "msg-abc123def456...",
  "status": "forwarded",
  "delivery_epoch": 1735500000,
  "relay_count": 3
}
```

#### Stored Message Format

Messages are stored with the following schema (see dchat-storage migrations):

```sql
-- New columns for secure envelope support
ALTER TABLE messages ADD COLUMN envelope_version INTEGER DEFAULT 0;
ALTER TABLE messages ADD COLUMN ciphertext BLOB;
ALTER TABLE messages ADD COLUMN wrapped_msg_key BLOB;
ALTER TABLE messages ADD COLUMN unlock_epoch INTEGER;
ALTER TABLE messages ADD COLUMN sender_ratchet_key BLOB;
ALTER TABLE messages ADD COLUMN message_index INTEGER;
ALTER TABLE messages ADD COLUMN flags INTEGER DEFAULT 0;

-- Epoch tokens table
CREATE TABLE epoch_tokens (
    id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    epoch_id INTEGER NOT NULL,
    token_blob BLOB NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(device_id, conversation_id, epoch_id)
);

-- Membership log table
CREATE TABLE membership_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload BLOB NOT NULL,
    prev_hash BLOB NOT NULL,
    merkle_root BLOB NOT NULL,
    authority_signature BLOB NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_membership_log_conv ON membership_log(conversation_id);
CREATE INDEX idx_epoch_tokens_expires ON epoch_tokens(expires_at);
```

#### Backward Compatibility

Messages with `envelope_version = 0` use the legacy format (plaintext content field). Clients MUST support both formats during transition:

```rust
match message.envelope_version {
    0 => {
        // Legacy: content is plaintext (or Noise-encrypted at transport)
        let plaintext = message.content;
    }
    1 => {
        // QGE v1: decrypt via SUK + epoch token
        let msg_key = unwrap_message_key(&message.wrapped_msg_key, &suk)?;
        let plaintext = aead_decrypt(&message.ciphertext, &msg_key)?;
    }
    _ => {
        return Err(Error::UnsupportedEnvelopeVersion);
    }
}
```

#### Decryption Flow

```
1. Check envelope_version == 1
2. Verify sender_signature over envelope
3. Obtain EpochToken for current epoch from quorum (if not cached)
4. Derive UK_t = HKDF(EpochToken, "unlock", conversation_id || device_id)
5. Decrypt SUK = AEAD_Decrypt(UK_t, encrypted_suk_blob)
6. Unwrap MessageKey = AEAD_Decrypt(SUK, wrapped_key, aad=message_id)
7. Decrypt plaintext = AEAD_Decrypt(MessageKey, ciphertext, aad=ratchet_header)
8. Advance ratchet state based on sender_ratchet_key and message_index
9. Increment decrypt budget counter
10. Mark message as read (optionally erase wrapped_key for forward secrecy)
```

---

#### `GET /messages`

**Purpose**: Retrieve messages for authenticated user

**Query Parameters**:

```
?channel_id=general
&limit=50
&offset=0
&since=1698518400000
&until=1698604800000
```

**Response (200 OK)**:

```json
{
  "messages": [
    {
      "id": "msg-abc123",
      "sender_id": "12D3KooWABC...",
      "recipient_id": "12D3KooWXYZ...",
      "channel_id": "general",
      "content": "Hello!",
      "timestamp_ms": 1698518400000,
      "status": "delivered",
      "reactions": ["👍", "❤️"]
    }
  ],
  "total_count": 1500,
  "has_more": true
}
```

---

#### `DELETE /messages/{message_id}`

**Purpose**: Delete message (if owner/admin)

**Response (204 No Content)**

**Error Responses**:

- `403 Forbidden` - Not owner/admin
- `404 Not Found` - Message doesn't exist

---

### Channel API

#### `POST /channels`

**Purpose**: Create a new channel

**Request**:

```json
{
  "name": "announcements",
  "description": "Important announcements",
  "privacy": "public",
  "members": ["12D3KooWABC...", "12D3KooWXYZ..."],
  "staking_required": 100
}
```

**Response (201 Created)**:

```json
{
  "channel_id": "chan-abc123",
  "name": "announcements",
  "created_timestamp": 1698518400000,
  "owner_id": "12D3KooWABC...",
  "member_count": 2
}
```

---

#### `GET /channels`

**Purpose**: List channels (paginated)

**Query Parameters**:

```
?limit=20
&offset=0
&sort=created_desc
```

**Response (200 OK)**:

```json
{
  "channels": [
    {
      "id": "chan-abc123",
      "name": "announcements",
      "description": "Important announcements",
      "member_count": 250,
      "last_message_timestamp": 1698518400000,
      "privacy": "public"
    }
  ],
  "total_count": 5000,
  "has_more": true
}
```

---

#### `GET /channels/{channel_id}/members`

**Purpose**: List channel members

**Response (200 OK)**:

```json
{
  "members": [
    {
      "peer_id": "12D3KooWABC...",
      "nickname": "Alice",
      "joined_timestamp": 1698518400000,
      "reputation_score": 85,
      "role": "owner"
    }
  ],
  "total_count": 250
}
```

---

### Identity API

#### `POST /identities`

**Purpose**: Register a new identity

**Request**:

```json
{
  "public_key": "7d8f9b6c...",
  "nickname": "Alice",
  "proof_of_device": "sgx_attestation_quote",
  "burner": false
}
```

**Response (201 Created)**:

```json
{
  "peer_id": "12D3KooWXYZ...",
  "public_key": "7d8f9b6c...",
  "reputation_score": 0,
  "created_timestamp": 1698518400000,
  "status": "active"
}
```

---

#### `GET /identities/{peer_id}`

**Purpose**: Get identity info

**Response (200 OK)**:

```json
{
  "peer_id": "12D3KooWABC...",
  "nickname": "Alice",
  "reputation_score": 85,
  "verified": true,
  "badges": ["founder", "verified_device"],
  "channels": 12,
  "joined_timestamp": 1698518400000
}
```

---

#### `POST /identities/{peer_id}/verify`

**Purpose**: Verify identity (get verified badge)

**Request**:

```json
{
  "proof_type": "device_attestation",
  "proof_data": "tpm_attestation_quote"
}
```

**Response (200 OK)**:

```json
{
  "verified": true,
  "badge_id": "badge-abc123",
  "verified_timestamp": 1698518400000
}
```

---

### Reputation API

#### `GET /reputation/{peer_id}`

**Purpose**: Get reputation score

**Response (200 OK)**:

```json
{
  "peer_id": "12D3KooWABC...",
  "score": 85,
  "level": "trusted",
  "components": {
    "message_delivery": 95,
    "governance_participation": 75,
    "uptime": 88,
    "moderation_votes": 80
  },
  "last_updated": 1698518400000
}
```

---

#### `POST /reputation/{peer_id}/challenge`

**Purpose**: Challenge reputation score (dispute resolution)

**Request**:

```json
{
  "claim": "Peer did not deliver message",
  "evidence": "hash_of_proof",
  "bond_amount": 1000
}
```

**Response (201 Created)**:

```json
{
  "dispute_id": "disp-abc123",
  "status": "open",
  "created_timestamp": 1698518400000,
  "challenge_period_ends": 1698604800000
}
```

---

### Governance API

#### `POST /governance/proposals`

**Purpose**: Submit governance proposal

**Request**:

```json
{
  "title": "Increase relay rewards",
  "description": "Increase rewards by 10% to attract more relays",
  "proposal_type": "parameter_change",
  "changes": {
    "relay_base_reward": 100000,
    "relay_uptime_bonus": 5000
  },
  "voting_period_seconds": 604800
}
```

**Response (201 Created)**:

```json
{
  "proposal_id": "prop-abc123",
  "status": "open",
  "voting_starts": 1698518400000,
  "voting_ends": 1699123200000,
  "votes_for": 1000,
  "votes_against": 200
}
```

---

#### `GET /governance/proposals`

**Purpose**: List proposals (paginated)

**Query Parameters**:

```
?status=open
&sort=votes_desc
&limit=20
```

**Response (200 OK)**:

```json
{
  "proposals": [
    {
      "id": "prop-abc123",
      "title": "Increase relay rewards",
      "status": "open",
      "votes_for": 1000,
      "votes_against": 200,
      "voting_percentage": 45.5,
      "voting_ends": 1699123200000
    }
  ],
  "total_count": 250,
  "has_more": true
}
```

---

#### `POST /governance/votes`

**Purpose**: Cast vote on proposal

**Request**:

```json
{
  "proposal_id": "prop-abc123",
  "vote": "yes",
  "stake": 1000
}
```

**Response (200 OK)**:

```json
{
  "vote_id": "vote-abc123",
  "proposal_id": "prop-abc123",
  "vote": "yes",
  "timestamp": 1698518400000,
  "power": 1000
}
```

---

### Marketplace API

#### `POST /marketplace/listings`

**Purpose**: Create digital good listing

**Request**:

```json
{
  "title": "Custom Emoji Pack",
  "description": "50 custom emojis",
  "price_tokens": 500,
  "type": "sticker_pack",
  "metadata": {
    "emoji_count": 50,
    "preview_images": ["ipfs://QmABC123..."]
  }
}
```

**Response (201 Created)**:

```json
{
  "listing_id": "list-abc123",
  "title": "Custom Emoji Pack",
  "price_tokens": 500,
  "created_timestamp": 1698518400000,
  "sales_count": 0
}
```

---

#### `GET /marketplace/listings`

**Purpose**: Browse marketplace

**Query Parameters**:

```
?type=sticker_pack
&sort=popularity
&limit=50
```

**Response (200 OK)**:

```json
{
  "listings": [
    {
      "id": "list-abc123",
      "title": "Custom Emoji Pack",
      "price_tokens": 500,
      "seller_id": "12D3KooWABC...",
      "sales_count": 1500,
      "rating": 4.8
    }
  ],
  "total_count": 10000,
  "has_more": true
}
```

---

#### `POST /marketplace/purchases`

**Purpose**: Purchase digital good (creates escrow)

**Request**:

```json
{
  "listing_id": "list-abc123",
  "quantity": 1
}
```

**Response (201 Created)**:

```json
{
  "purchase_id": "purch-abc123",
  "listing_id": "list-abc123",
  "amount_tokens": 500,
  "status": "escrow_held",
  "expires_timestamp": 1698604800000
}
```

---

## P2P/libp2p API

### Protocols

dchat uses libp2p with custom protocols:

#### Message Protocol (`/dchat/msg/1.0.0`)

**Purpose**: Send messages between peers

**Message Format**:

```protobuf
message PeerMessage {
  string id = 1;
  string sender_id = 2;
  string recipient_id = 3;
  bytes content = 4;
  int64 timestamp_ms = 5;
  MessageStatus status = 6;
}
```

**Handlers**:

```rust
// Handler must respond with delivery confirmation
async fn handle_message(msg: PeerMessage) -> Result<DeliveryConfirmation> {
    // Process message
    Ok(DeliveryConfirmation {
        message_id: msg.id,
        status: "delivered",
        timestamp_ms: now(),
    })
}
```

---

#### Gossip Protocol (`/dchat/gossip/1.0.0`)

**Purpose**: Epidemic message propagation with bloom filter dedup

**Configuration**:

```rust
let config = GossipConfig {
    fanout: 6,
    message_cache_size: 5000,
    max_ttl: 3,
    cache_ttl_ms: 120000,
};
```

---

#### DHT Discovery (`/dchat/dht/1.0.0`)

**Purpose**: Kademlia peer discovery

**Operations**:

- `FindPeer(peer_id)` - Locate peer in DHT
- `FindProviders(content_hash)` - Find peers with content
- `GetClosestPeers(key)` - Get K closest peers to key

---

#### Connection Management Protocol

**Purpose**: Manage peer connections, health checks

**Heartbeat** (every 30 seconds):

```json
{
  "type": "heartbeat",
  "peer_id": "12D3KooWABC...",
  "timestamp_ms": 1698518400000,
  "status": "healthy",
  "message_count": 1500
}
```

---

## gRPC API

### Service Definition (`dchat.proto`)

```protobuf
service DchatService {
  // Messages
  rpc SendMessage(SendMessageRequest) returns (SendMessageResponse);
  rpc ReceiveMessages(ReceiveMessagesRequest) returns (stream Message);

  // Channels
  rpc CreateChannel(CreateChannelRequest) returns (ChannelInfo);
  rpc ListChannels(ListChannelsRequest) returns (ChannelList);
  rpc JoinChannel(JoinChannelRequest) returns (ChannelMembership);
  rpc LeaveChannel(LeaveChannelRequest) returns (Empty);

  // Identity
  rpc RegisterIdentity(RegisterIdentityRequest) returns (IdentityInfo);
  rpc GetIdentity(GetIdentityRequest) returns (IdentityInfo);
  rpc UpdateProfile(UpdateProfileRequest) returns (IdentityInfo);

  // Governance
  rpc CreateProposal(CreateProposalRequest) returns (ProposalInfo);
  rpc ListProposals(ListProposalsRequest) returns (ProposalList);
  rpc CastVote(CastVoteRequest) returns (VoteInfo);

  // Marketplace
  rpc ListMarketplaceItems(ListItemsRequest) returns (ItemList);
  rpc CreateListing(CreateListingRequest) returns (ListingInfo);
  rpc Purchase(PurchaseRequest) returns (PurchaseInfo);
}
```

### Example Usage (Rust)

```rust
use dchat_grpc::DchatServiceClient;

#[tokio::main]
async fn main() -> Result<()> {
    let mut client = DchatServiceClient::connect("http://[::1]:50051").await?;

    // Send message
    let request = SendMessageRequest {
        recipient_id: "12D3KooWXYZ...".to_string(),
        content: "Hello!".to_string(),
        channel_id: "general".to_string(),
    };

    let response = client.send_message(request).await?;
    println!("Message sent: {:?}", response);

    Ok(())
}
```

### Example Usage (Python)

```python
import grpc
from dchat_pb2 import SendMessageRequest
from dchat_pb2_grpc import DchatServiceStub

async def send_message():
    async with grpc.aio.secure_channel('localhost:50051', ...) as channel:
        stub = DchatServiceStub(channel)
        request = SendMessageRequest(
            recipient_id='12D3KooWXYZ...',
            content='Hello!',
            channel_id='general'
        )
        response = await stub.SendMessage(request)
        print(f"Message sent: {response}")
```

---

## WebSocket API

### Connection

```javascript
// Connect to WebSocket endpoint
const ws = new WebSocket("wss://localhost:8080/ws");

ws.onopen = () => {
  console.log("Connected");

  // Authenticate
  ws.send(
    JSON.stringify({
      type: "auth",
      token: "bearer_token_here",
    }),
  );
};
```

### Message Events

#### Subscribe to Messages

```javascript
ws.send(
  JSON.stringify({
    type: "subscribe",
    channel: "general",
  }),
);

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  if (msg.type === "message") {
    console.log(`${msg.sender}: ${msg.content}`);
  }
};
```

#### Send Message

```javascript
ws.send(
  JSON.stringify({
    type: "message",
    recipient_id: "12D3KooWXYZ...",
    content: "Hello!",
    channel_id: "general",
  }),
);
```

#### Subscribe to Events

```javascript
ws.send(
  JSON.stringify({
    type: "subscribe",
    events: ["reputation_changed", "proposal_created", "governance_vote"],
  }),
);
```

---

## Error Handling

### HTTP Error Response Format

```json
{
  "error": "INVALID_MESSAGE",
  "message": "Message content exceeds maximum length",
  "code": 400,
  "timestamp": 1698518400000,
  "request_id": "req-abc123def456"
}
```

### Error Codes

| Code  | Status | Meaning             |
| ----- | ------ | ------------------- |
| `100` | 400    | INVALID_REQUEST     |
| `101` | 400    | INVALID_MESSAGE     |
| `102` | 400    | INVALID_IDENTITY    |
| `200` | 401    | UNAUTHORIZED        |
| `201` | 403    | FORBIDDEN           |
| `300` | 404    | NOT_FOUND           |
| `301` | 404    | CHANNEL_NOT_FOUND   |
| `302` | 404    | MESSAGE_NOT_FOUND   |
| `400` | 429    | RATE_LIMITED        |
| `500` | 500    | INTERNAL_ERROR      |
| `501` | 503    | SERVICE_UNAVAILABLE |

### Error Examples

#### Invalid Message

```json
{
  "error": "INVALID_MESSAGE",
  "message": "Content cannot be empty",
  "code": 400
}
```

#### Rate Limited

```json
{
  "error": "RATE_LIMITED",
  "message": "Too many requests. Retry after 60 seconds",
  "code": 429,
  "retry_after_seconds": 60
}
```

#### Service Unavailable

```json
{
  "error": "SERVICE_UNAVAILABLE",
  "message": "Relay overloaded, message queued for later delivery",
  "code": 503
}
```

---

## Rate Limiting

### Reputation-Based QoS

Rate limits depend on peer reputation score (0-100):

| Reputation | Messages/Min | Channels/Hour | Proposals/Month |
| ---------- | ------------ | ------------- | --------------- |
| 0-20       | 5            | 1             | 0 (blocked)     |
| 21-50      | 20           | 5             | 1               |
| 51-80      | 100          | 20            | 5               |
| 81-100     | 500          | 100           | 20              |

### Rate Limit Headers

```
HTTP/1.1 200 OK
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 45
X-RateLimit-Reset: 1698518460
```

### Rate Limit Response (429)

```json
{
  "error": "RATE_LIMITED",
  "limit": 100,
  "remaining": 0,
  "reset_timestamp": 1698518460000
}
```

---

## Authentication

### Bearer Token (Recommended)

```bash
curl -H "Authorization: Bearer <token>" \
  http://localhost:8080/api/v1/messages
```

### Ed25519 Signature (P2P)

For P2P communication, sign requests with Ed25519:

```rust
let message = b"GET /messages";
let signature = keypair.sign(message);

// Send with signature
let signed_request = SignedRequest {
    request: message.to_vec(),
    peer_id: self.peer_id.clone(),
    timestamp_ms: now(),
    signature: signature.to_bytes().to_vec(),
};
```

### Token Generation

```bash
dchat auth generate-token \
  --identity /path/to/identity.json \
  --expiry 3600
```

---

## Versioning

### API Versioning Strategy

**Current Version**: `v1`  
**URL Pattern**: `/api/v1/...`

### Version Negotiation

Clients specify API version in requests:

```bash
curl -H "Accept: application/json; version=v1" \
  http://localhost:8080/api/messages
```

### Breaking Changes

Breaking changes increment major version (v1 → v2):

- Changes to request/response format
- Removal of endpoints
- Behavior changes

Non-breaking changes (v1.1, v1.2) include:

- New endpoints
- New optional parameters
- New response fields

### Deprecation Timeline

When deprecating an API:

1. **Announce** (1 month notice)
2. **Deprecate** (mark as deprecated in docs, add header)
3. **Support** (6 months dual support)
4. **Remove** (fully remove old API)

### Deprecation Header

```
HTTP/1.1 200 OK
Deprecation: true
Sunset: Sun, 01 Apr 2026 00:00:00 GMT
Link: </api/v2/messages>; rel="successor-version"
```

---

## SDK Examples

### Rust SDK

```rust
use dchat_sdk::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new()
        .identity("./identity.json")
        .bootstrap_peers(vec![
            "/ip4/seed1.dchat.org/tcp/7070/p2p/12D3KooWABC..."
        ])
        .build()
        .await?;

    // Send message
    let msg = client.send_message(
        "12D3KooWXYZ...",
        "Hello, world!"
    ).await?;

    println!("Message sent: {}", msg.id);

    Ok(())
}
```

### TypeScript SDK

```typescript
import { DchatClient } from "@dchat/sdk";

const client = new DchatClient({
  identity: "./identity.json",
  bootstrapPeers: ["/ip4/seed1.dchat.org/tcp/7070/p2p/12D3KooWABC..."],
});

// Send message
const msg = await client.sendMessage({
  recipientId: "12D3KooWXYZ...",
  content: "Hello, world!",
  channelId: "general",
});

console.log(`Message sent: ${msg.id}`);

// Subscribe to messages
client.on("message", (msg) => {
  console.log(`${msg.sender}: ${msg.content}`);
});
```

### Python SDK

```python
from dchat import Client

client = Client(
    identity='./identity.json',
    bootstrap_peers=['/ip4/seed1.dchat.org/tcp/7070/p2p/12D3KooWABC...']
)

# Send message
msg = client.send_message(
    recipient_id='12D3KooWXYZ...',
    content='Hello, world!',
    channel_id='general'
)

print(f'Message sent: {msg.id}')

# Receive messages
for msg in client.receive_messages():
    print(f'{msg.sender}: {msg.content}')
```

---

## Appendix

### Default Ports

| Service            | Port  | Protocol |
| ------------------ | ----- | -------- |
| P2P                | 7070  | TCP      |
| HTTP API           | 8080  | HTTP     |
| gRPC               | 50051 | gRPC     |
| Prometheus Metrics | 9090  | HTTP     |

### Content Type Headers

```
Content-Type: application/json
Content-Type: application/grpc
Content-Type: application/grpc+proto
```

### Common Query Parameters

```
?limit=50            # Pagination limit
?offset=0            # Pagination offset
?sort=created_desc   # Sort order
?filter=active       # Filter results
?since=1698518400000 # Timestamp filters
&until=1698604800000
```

---

**End of API Specification**

For SDKs and client libraries, see:

- Rust: https://github.com/dchat/sdk-rust
- TypeScript: https://github.com/dchat/sdk-typescript
- Python: https://github.com/dchat/sdk-python

For gRPC definitions:

- https://github.com/dchat/protos

Version: 1.0 | Last Updated: October 28, 2025
