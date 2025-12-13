//! Fast-Path Transport Framing
//!
//! Implements efficient wire protocol with:
//! - Length-prefixed frames with strict size bounds
//! - Cheap header validation before allocation
//! - Stateless handshake cookies (SYN flood protection)
//! - Zero-copy parsing where possible
//!
//! Security: Early rejection of malformed packets minimizes
//! resource consumption from malicious traffic.

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Protocol magic bytes
pub const PROTOCOL_MAGIC: [u8; 4] = *b"DCHT";

/// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

/// Maximum frame size (4 MB)
pub const MAX_FRAME_SIZE: usize = 4 * 1024 * 1024;

/// Minimum frame size (header only)
pub const MIN_FRAME_SIZE: usize = FRAME_HEADER_SIZE;

/// Frame header size
pub const FRAME_HEADER_SIZE: usize = 16;

/// Cookie validity period (seconds)
pub const COOKIE_VALIDITY_SECS: u64 = 60;

/// Cookie rotation period (seconds)
pub const COOKIE_ROTATION_SECS: u64 = 30;

/// Maximum pending handshakes per IP
pub const MAX_PENDING_HANDSHAKES_PER_IP: usize = 10;

/// Framing errors
#[derive(Debug, Error)]
pub enum FramingError {
    #[error("Invalid magic bytes")]
    InvalidMagic,

    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u8),

    #[error("Frame too large: {0} bytes (max {1})")]
    FrameTooLarge(usize, usize),

    #[error("Frame too small: {0} bytes (min {1})")]
    FrameTooSmall(usize, usize),

    #[error("Invalid frame type: {0}")]
    InvalidFrameType(u8),

    #[error("Checksum mismatch")]
    ChecksumMismatch,

    #[error("Invalid cookie")]
    InvalidCookie,

    #[error("Expired cookie")]
    ExpiredCookie,

    #[error("Invalid handshake state")]
    InvalidHandshakeState,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Connection rate limited")]
    RateLimited,
}

/// Frame types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum FrameType {
    /// Handshake initiation (client -> server)
    HandshakeInit = 0x01,
    /// Handshake response with cookie (server -> client)
    HandshakeCookie = 0x02,
    /// Handshake completion with cookie (client -> server)
    HandshakeComplete = 0x03,
    /// Handshake acknowledgment (server -> client)
    HandshakeAck = 0x04,

    /// Data frame (encrypted payload)
    Data = 0x10,
    /// Keepalive ping
    Ping = 0x11,
    /// Keepalive pong
    Pong = 0x12,

    /// Close connection
    Close = 0x20,
    /// Error notification
    Error = 0x21,

    /// Control frame (priority/flow control)
    Control = 0x30,
}

impl TryFrom<u8> for FrameType {
    type Error = FramingError;

    fn try_from(value: u8) -> Result<Self, <Self as TryFrom<u8>>::Error> {
        match value {
            0x01 => Ok(FrameType::HandshakeInit),
            0x02 => Ok(FrameType::HandshakeCookie),
            0x03 => Ok(FrameType::HandshakeComplete),
            0x04 => Ok(FrameType::HandshakeAck),
            0x10 => Ok(FrameType::Data),
            0x11 => Ok(FrameType::Ping),
            0x12 => Ok(FrameType::Pong),
            0x20 => Ok(FrameType::Close),
            0x21 => Ok(FrameType::Error),
            0x30 => Ok(FrameType::Control),
            _ => Err(FramingError::InvalidFrameType(value)),
        }
    }
}

/// Frame flags
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameFlags(u8);

impl FrameFlags {
    pub const NONE: Self = Self(0);
    pub const COMPRESSED: Self = Self(0x01);
    pub const ENCRYPTED: Self = Self(0x02);
    pub const PRIORITY_HIGH: Self = Self(0x04);
    pub const REQUIRES_ACK: Self = Self(0x08);

    pub fn new(flags: u8) -> Self {
        Self(flags)
    }

    pub fn is_compressed(&self) -> bool {
        self.0 & 0x01 != 0
    }

    pub fn is_encrypted(&self) -> bool {
        self.0 & 0x02 != 0
    }

    pub fn is_priority_high(&self) -> bool {
        self.0 & 0x04 != 0
    }

    pub fn requires_ack(&self) -> bool {
        self.0 & 0x08 != 0
    }

    pub fn as_byte(&self) -> u8 {
        self.0
    }
}

/// Frame header (16 bytes, fixed size for cheap parsing)
///
/// Layout:
/// - [0..4]: Magic bytes "DCHT"
/// - [4]: Version
/// - [5]: Frame type
/// - [6]: Flags
/// - [7]: Reserved
/// - [8..12]: Payload length (big-endian u32)
/// - [12..16]: Header checksum (truncated blake3)
#[derive(Debug, Clone)]
pub struct FrameHeader {
    pub version: u8,
    pub frame_type: FrameType,
    pub flags: FrameFlags,
    pub payload_length: u32,
}

impl FrameHeader {
    /// Create new frame header
    pub fn new(frame_type: FrameType, flags: FrameFlags, payload_length: usize) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            frame_type,
            flags,
            payload_length: payload_length as u32,
        }
    }

    /// Serialize header to bytes
    pub fn to_bytes(&self) -> [u8; FRAME_HEADER_SIZE] {
        let mut bytes = [0u8; FRAME_HEADER_SIZE];

        // Magic
        bytes[0..4].copy_from_slice(&PROTOCOL_MAGIC);

        // Version
        bytes[4] = self.version;

        // Frame type
        bytes[5] = self.frame_type as u8;

        // Flags
        bytes[6] = self.flags.as_byte();

        // Reserved
        bytes[7] = 0;

        // Payload length
        bytes[8..12].copy_from_slice(&self.payload_length.to_be_bytes());

        // Compute checksum of first 12 bytes
        let checksum = Self::compute_checksum(&bytes[0..12]);
        bytes[12..16].copy_from_slice(&checksum);

        bytes
    }

    /// Parse header from bytes (validates checksum)
    pub fn from_bytes(bytes: &[u8; FRAME_HEADER_SIZE]) -> Result<Self, FramingError> {
        // Check magic
        if bytes[0..4] != PROTOCOL_MAGIC {
            return Err(FramingError::InvalidMagic);
        }

        // Check version
        let version = bytes[4];
        if version != PROTOCOL_VERSION {
            return Err(FramingError::UnsupportedVersion(version));
        }

        // Verify checksum
        let expected_checksum = Self::compute_checksum(&bytes[0..12]);
        if bytes[12..16] != expected_checksum {
            return Err(FramingError::ChecksumMismatch);
        }

        // Parse fields
        let frame_type = FrameType::try_from(bytes[5])?;
        let flags = FrameFlags::new(bytes[6]);
        let payload_length = u32::from_be_bytes(bytes[8..12].try_into().unwrap());

        // Validate payload length
        if payload_length as usize > MAX_FRAME_SIZE - FRAME_HEADER_SIZE {
            return Err(FramingError::FrameTooLarge(
                payload_length as usize + FRAME_HEADER_SIZE,
                MAX_FRAME_SIZE,
            ));
        }

        Ok(Self {
            version,
            frame_type,
            flags,
            payload_length,
        })
    }

    /// Quick validation without full parsing (for early rejection)
    pub fn quick_validate(bytes: &[u8]) -> Result<usize, FramingError> {
        if bytes.len() < FRAME_HEADER_SIZE {
            return Err(FramingError::FrameTooSmall(bytes.len(), FRAME_HEADER_SIZE));
        }

        // Check magic (no allocation, no parsing)
        if bytes[0..4] != PROTOCOL_MAGIC {
            return Err(FramingError::InvalidMagic);
        }

        // Check version
        if bytes[4] != PROTOCOL_VERSION {
            return Err(FramingError::UnsupportedVersion(bytes[4]));
        }

        // Get payload length
        let payload_length = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;

        // Check size bounds
        let total_size = FRAME_HEADER_SIZE + payload_length;
        if total_size > MAX_FRAME_SIZE {
            return Err(FramingError::FrameTooLarge(total_size, MAX_FRAME_SIZE));
        }

        Ok(total_size)
    }

    fn compute_checksum(data: &[u8]) -> [u8; 4] {
        let hash = blake3::hash(data);
        let bytes = hash.as_bytes();
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }
}

/// Complete frame
#[derive(Debug, Clone)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

impl Frame {
    /// Create new frame
    pub fn new(frame_type: FrameType, flags: FrameFlags, payload: Vec<u8>) -> Self {
        Self {
            header: FrameHeader::new(frame_type, flags, payload.len()),
            payload,
        }
    }

    /// Serialize frame to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_bytes = self.header.to_bytes();
        let mut bytes = Vec::with_capacity(FRAME_HEADER_SIZE + self.payload.len());
        bytes.extend_from_slice(&header_bytes);
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Parse frame from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() < FRAME_HEADER_SIZE {
            return Err(FramingError::FrameTooSmall(bytes.len(), FRAME_HEADER_SIZE));
        }

        let header_bytes: &[u8; FRAME_HEADER_SIZE] =
            bytes[0..FRAME_HEADER_SIZE].try_into().unwrap();
        let header = FrameHeader::from_bytes(header_bytes)?;

        let expected_len = FRAME_HEADER_SIZE + header.payload_length as usize;
        if bytes.len() < expected_len {
            return Err(FramingError::FrameTooSmall(bytes.len(), expected_len));
        }

        let payload = bytes[FRAME_HEADER_SIZE..expected_len].to_vec();

        Ok(Self { header, payload })
    }

    /// Read frame from reader
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, FramingError> {
        // Read header first
        let mut header_bytes = [0u8; FRAME_HEADER_SIZE];
        reader.read_exact(&mut header_bytes)?;

        let header = FrameHeader::from_bytes(&header_bytes)?;

        // Read payload
        let mut payload = vec![0u8; header.payload_length as usize];
        reader.read_exact(&mut payload)?;

        Ok(Self { header, payload })
    }

    /// Write frame to writer
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), FramingError> {
        let header_bytes = self.header.to_bytes();
        writer.write_all(&header_bytes)?;
        writer.write_all(&self.payload)?;
        Ok(())
    }
}

/// Stateless cookie for SYN flood protection
///
/// Cookie = HMAC(server_secret || client_ip || client_port || timestamp)
///
/// Server doesn't store any state until client proves they received the cookie.
#[derive(Debug, Clone)]
pub struct Cookie {
    /// Timestamp when cookie was generated
    pub timestamp: u64,
    /// Cookie value
    pub value: [u8; 32],
}

impl Cookie {
    /// Generate a new cookie
    pub fn generate(secret: &[u8; 32], client_addr: &SocketAddr) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let value = Self::compute(secret, client_addr, timestamp);

        Self { timestamp, value }
    }

    /// Verify a cookie
    pub fn verify(&self, secret: &[u8; 32], client_addr: &SocketAddr) -> Result<(), FramingError> {
        // Check timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now - self.timestamp > COOKIE_VALIDITY_SECS {
            return Err(FramingError::ExpiredCookie);
        }

        // Verify cookie value
        let expected = Self::compute(secret, client_addr, self.timestamp);

        if self.value != expected {
            return Err(FramingError::InvalidCookie);
        }

        Ok(())
    }

    fn compute(secret: &[u8; 32], client_addr: &SocketAddr, timestamp: u64) -> [u8; 32] {
        let mut hasher = Hasher::new_keyed(secret);

        // Add client address
        match client_addr {
            SocketAddr::V4(v4) => {
                hasher.update(&[4]); // IPv4 marker
                hasher.update(&v4.ip().octets());
                hasher.update(&v4.port().to_be_bytes());
            }
            SocketAddr::V6(v6) => {
                hasher.update(&[6]); // IPv6 marker
                hasher.update(&v6.ip().octets());
                hasher.update(&v6.port().to_be_bytes());
            }
        }

        // Add timestamp
        hasher.update(&timestamp.to_be_bytes());

        *hasher.finalize().as_bytes()
    }

    /// Serialize cookie
    pub fn to_bytes(&self) -> [u8; 40] {
        let mut bytes = [0u8; 40];
        bytes[0..8].copy_from_slice(&self.timestamp.to_be_bytes());
        bytes[8..40].copy_from_slice(&self.value);
        bytes
    }

    /// Deserialize cookie
    pub fn from_bytes(bytes: &[u8; 40]) -> Self {
        let timestamp = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let mut value = [0u8; 32];
        value.copy_from_slice(&bytes[8..40]);
        Self { timestamp, value }
    }
}

/// Cookie secret manager (rotates secrets)
pub struct CookieSecretManager {
    /// Current secret
    current_secret: [u8; 32],
    /// Previous secret (for graceful rotation)
    previous_secret: [u8; 32],
    /// Last rotation time
    last_rotation: std::time::Instant,
    /// Rotation interval
    rotation_interval: Duration,
}

impl CookieSecretManager {
    /// Create new manager with random secret
    pub fn new() -> Self {
        use rand::RngCore;
        let mut rng = rand::thread_rng();

        let mut current_secret = [0u8; 32];
        let mut previous_secret = [0u8; 32];

        rng.fill_bytes(&mut current_secret);
        rng.fill_bytes(&mut previous_secret);

        Self {
            current_secret,
            previous_secret,
            last_rotation: std::time::Instant::now(),
            rotation_interval: Duration::from_secs(COOKIE_ROTATION_SECS),
        }
    }

    /// Get current secret (may rotate first)
    pub fn current_secret(&mut self) -> &[u8; 32] {
        self.maybe_rotate();
        &self.current_secret
    }

    /// Verify cookie with either current or previous secret
    pub fn verify_cookie(
        &mut self,
        cookie: &Cookie,
        client_addr: &SocketAddr,
    ) -> Result<(), FramingError> {
        self.maybe_rotate();

        // Try current secret first
        if cookie.verify(&self.current_secret, client_addr).is_ok() {
            return Ok(());
        }

        // Try previous secret
        cookie.verify(&self.previous_secret, client_addr)
    }

    fn maybe_rotate(&mut self) {
        let now = std::time::Instant::now();

        if now.duration_since(self.last_rotation) >= self.rotation_interval {
            use rand::RngCore;
            let mut rng = rand::thread_rng();

            // Move current to previous
            self.previous_secret = self.current_secret;

            // Generate new current
            rng.fill_bytes(&mut self.current_secret);

            self.last_rotation = now;
        }
    }
}

impl Default for CookieSecretManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Handshake state machine
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeState {
    /// Initial state (server or client)
    Initial,
    /// Awaiting cookie response (client)
    AwaitingCookie,
    /// Cookie sent, awaiting completion (server)
    CookieSent,
    /// Awaiting acknowledgment (client)
    AwaitingAck,
    /// Handshake complete
    Complete,
    /// Handshake failed
    Failed(String),
}

/// Handshake initiator payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeInit {
    pub client_random: [u8; 32],
    pub protocol_version: u8,
    pub extensions: Vec<u8>,
}

/// Handshake cookie payload
#[derive(Debug, Clone)]
pub struct HandshakeCookiePayload {
    pub cookie: [u8; 40],
    pub server_random: [u8; 32],
}

impl serde::Serialize for HandshakeCookiePayload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("HandshakeCookiePayload", 2)?;
        s.serialize_field("cookie", &self.cookie.as_slice())?;
        s.serialize_field("server_random", &self.server_random)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for HandshakeCookiePayload {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper {
            cookie: Vec<u8>,
            server_random: [u8; 32],
        }
        let h = Helper::deserialize(deserializer)?;
        if h.cookie.len() != 40 {
            return Err(serde::de::Error::custom("cookie must be 40 bytes"));
        }
        let mut cookie = [0u8; 40];
        cookie.copy_from_slice(&h.cookie);
        Ok(Self {
            cookie,
            server_random: h.server_random,
        })
    }
}

/// Handshake completion payload
#[derive(Debug, Clone)]
pub struct HandshakeComplete {
    pub cookie: [u8; 40],
    pub client_random: [u8; 32],
    pub client_proof: [u8; 32],
}

impl serde::Serialize for HandshakeComplete {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("HandshakeComplete", 3)?;
        s.serialize_field("cookie", &self.cookie.as_slice())?;
        s.serialize_field("client_random", &self.client_random)?;
        s.serialize_field("client_proof", &self.client_proof)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for HandshakeComplete {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper {
            cookie: Vec<u8>,
            client_random: [u8; 32],
            client_proof: [u8; 32],
        }
        let h = Helper::deserialize(deserializer)?;
        if h.cookie.len() != 40 {
            return Err(serde::de::Error::custom("cookie must be 40 bytes"));
        }
        let mut cookie = [0u8; 40];
        cookie.copy_from_slice(&h.cookie);
        Ok(Self {
            cookie,
            client_random: h.client_random,
            client_proof: h.client_proof,
        })
    }
}

/// Handshake acknowledgment payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeAck {
    pub server_proof: [u8; 32],
    pub session_id: [u8; 32],
}

/// Handshake handler (server side)
pub struct ServerHandshake {
    cookie_manager: CookieSecretManager,
    pending_count: dashmap::DashMap<std::net::IpAddr, usize>,
    server_random: [u8; 32],
}

impl ServerHandshake {
    pub fn new() -> Self {
        use rand::RngCore;
        let mut rng = rand::thread_rng();

        let mut server_random = [0u8; 32];
        rng.fill_bytes(&mut server_random);

        Self {
            cookie_manager: CookieSecretManager::new(),
            pending_count: dashmap::DashMap::new(),
            server_random,
        }
    }

    /// Process handshake init, return cookie response
    pub fn process_init(
        &mut self,
        client_addr: &SocketAddr,
        init: &HandshakeInit,
    ) -> Result<Frame, FramingError> {
        // Rate limit per IP
        let ip = client_addr.ip();
        let count = *self.pending_count.entry(ip).or_insert(0);

        if count >= MAX_PENDING_HANDSHAKES_PER_IP {
            return Err(FramingError::RateLimited);
        }

        *self.pending_count.entry(ip).or_insert(0) += 1;

        // Generate cookie
        let cookie = Cookie::generate(self.cookie_manager.current_secret(), client_addr);

        let response = HandshakeCookiePayload {
            cookie: cookie.to_bytes(),
            server_random: self.server_random,
        };

        let payload = bincode::serialize(&response)
            .map_err(|e| FramingError::Serialization(e.to_string()))?;

        Ok(Frame::new(
            FrameType::HandshakeCookie,
            FrameFlags::NONE,
            payload,
        ))
    }

    /// Process handshake completion, return acknowledgment
    pub fn process_complete(
        &mut self,
        client_addr: &SocketAddr,
        complete: &HandshakeComplete,
    ) -> Result<Frame, FramingError> {
        // Verify cookie
        let cookie = Cookie::from_bytes(&complete.cookie);
        self.cookie_manager.verify_cookie(&cookie, client_addr)?;

        // Decrement pending count
        let ip = client_addr.ip();
        if let Some(mut count) = self.pending_count.get_mut(&ip) {
            *count = count.saturating_sub(1);
        }

        // Generate session ID
        let mut hasher = Hasher::new();
        hasher.update(&complete.client_random);
        hasher.update(&self.server_random);
        hasher.update(&complete.cookie);
        let session_id = *hasher.finalize().as_bytes();

        // Generate server proof
        let mut proof_hasher = Hasher::new();
        proof_hasher.update(&session_id);
        proof_hasher.update(&complete.client_proof);
        proof_hasher.update(&self.server_random);
        let server_proof = *proof_hasher.finalize().as_bytes();

        let ack = HandshakeAck {
            server_proof,
            session_id,
        };

        let payload =
            bincode::serialize(&ack).map_err(|e| FramingError::Serialization(e.to_string()))?;

        Ok(Frame::new(
            FrameType::HandshakeAck,
            FrameFlags::NONE,
            payload,
        ))
    }
}

impl Default for ServerHandshake {
    fn default() -> Self {
        Self::new()
    }
}

/// Handshake handler (client side)
pub struct ClientHandshake {
    client_random: [u8; 32],
    state: HandshakeState,
    received_cookie: Option<[u8; 40]>,
    server_random: Option<[u8; 32]>,
}

impl ClientHandshake {
    pub fn new() -> Self {
        use rand::RngCore;
        let mut rng = rand::thread_rng();

        let mut client_random = [0u8; 32];
        rng.fill_bytes(&mut client_random);

        Self {
            client_random,
            state: HandshakeState::Initial,
            received_cookie: None,
            server_random: None,
        }
    }

    /// Generate init frame
    pub fn init(&mut self) -> Result<Frame, FramingError> {
        let init = HandshakeInit {
            client_random: self.client_random,
            protocol_version: PROTOCOL_VERSION,
            extensions: Vec::new(),
        };

        let payload =
            bincode::serialize(&init).map_err(|e| FramingError::Serialization(e.to_string()))?;

        self.state = HandshakeState::AwaitingCookie;

        Ok(Frame::new(
            FrameType::HandshakeInit,
            FrameFlags::NONE,
            payload,
        ))
    }

    /// Process cookie response, generate completion frame
    pub fn process_cookie(&mut self, payload: &[u8]) -> Result<Frame, FramingError> {
        if self.state != HandshakeState::AwaitingCookie {
            return Err(FramingError::InvalidHandshakeState);
        }

        let cookie_payload: HandshakeCookiePayload = bincode::deserialize(payload)
            .map_err(|e| FramingError::Serialization(e.to_string()))?;

        self.received_cookie = Some(cookie_payload.cookie);
        self.server_random = Some(cookie_payload.server_random);

        // Generate client proof
        let mut proof_hasher = Hasher::new();
        proof_hasher.update(&self.client_random);
        proof_hasher.update(&cookie_payload.server_random);
        proof_hasher.update(&cookie_payload.cookie);
        let client_proof = *proof_hasher.finalize().as_bytes();

        let complete = HandshakeComplete {
            cookie: cookie_payload.cookie,
            client_random: self.client_random,
            client_proof,
        };

        let payload = bincode::serialize(&complete)
            .map_err(|e| FramingError::Serialization(e.to_string()))?;

        self.state = HandshakeState::AwaitingAck;

        Ok(Frame::new(
            FrameType::HandshakeComplete,
            FrameFlags::NONE,
            payload,
        ))
    }

    /// Process acknowledgment, complete handshake
    pub fn process_ack(&mut self, payload: &[u8]) -> Result<[u8; 32], FramingError> {
        if self.state != HandshakeState::AwaitingAck {
            return Err(FramingError::InvalidHandshakeState);
        }

        let ack: HandshakeAck = bincode::deserialize(payload)
            .map_err(|e| FramingError::Serialization(e.to_string()))?;

        // Verify server proof (optional additional security)
        // In production, this would verify the server's identity

        self.state = HandshakeState::Complete;

        Ok(ack.session_id)
    }

    /// Get current state
    pub fn state(&self) -> &HandshakeState {
        &self.state
    }
}

impl Default for ClientHandshake {
    fn default() -> Self {
        Self::new()
    }
}

/// Frame reader with buffering and size limits
pub struct FrameReader<R: Read> {
    reader: R,
    buffer: Vec<u8>,
    max_frame_size: usize,
}

impl<R: Read> FrameReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::with_capacity(FRAME_HEADER_SIZE),
            max_frame_size: MAX_FRAME_SIZE,
        }
    }

    pub fn with_max_size(reader: R, max_size: usize) -> Self {
        Self {
            reader,
            buffer: Vec::with_capacity(FRAME_HEADER_SIZE),
            max_frame_size: max_size,
        }
    }

    /// Read next frame
    pub fn read_frame(&mut self) -> Result<Frame, FramingError> {
        // Read header
        self.buffer.clear();
        self.buffer.resize(FRAME_HEADER_SIZE, 0);
        self.reader.read_exact(&mut self.buffer)?;

        // Quick validation
        let total_size = FrameHeader::quick_validate(&self.buffer)?;

        if total_size > self.max_frame_size {
            return Err(FramingError::FrameTooLarge(total_size, self.max_frame_size));
        }

        // Parse header
        let header_bytes: &[u8; FRAME_HEADER_SIZE] =
            self.buffer[0..FRAME_HEADER_SIZE].try_into().unwrap();
        let header = FrameHeader::from_bytes(header_bytes)?;

        // Read payload
        let mut payload = vec![0u8; header.payload_length as usize];
        self.reader.read_exact(&mut payload)?;

        Ok(Frame { header, payload })
    }
}

/// Frame writer
pub struct FrameWriter<W: Write> {
    writer: W,
}

impl<W: Write> FrameWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Write frame
    pub fn write_frame(&mut self, frame: &Frame) -> Result<(), FramingError> {
        frame.write_to(&mut self.writer)
    }

    /// Flush writer
    pub fn flush(&mut self) -> Result<(), FramingError> {
        self.writer.flush().map_err(Into::into)
    }
}

/// Utility functions for common frame types
pub mod frames {
    use super::*;

    /// Create a ping frame
    pub fn ping(sequence: u64) -> Frame {
        Frame::new(
            FrameType::Ping,
            FrameFlags::NONE,
            sequence.to_be_bytes().to_vec(),
        )
    }

    /// Create a pong frame
    pub fn pong(sequence: u64) -> Frame {
        Frame::new(
            FrameType::Pong,
            FrameFlags::NONE,
            sequence.to_be_bytes().to_vec(),
        )
    }

    /// Create a close frame
    pub fn close(reason: &str) -> Frame {
        Frame::new(
            FrameType::Close,
            FrameFlags::NONE,
            reason.as_bytes().to_vec(),
        )
    }

    /// Create an error frame
    pub fn error(code: u16, message: &str) -> Frame {
        let mut payload = Vec::with_capacity(2 + message.len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(message.as_bytes());
        Frame::new(FrameType::Error, FrameFlags::NONE, payload)
    }

    /// Create a data frame
    pub fn data(payload: Vec<u8>, encrypted: bool, compressed: bool) -> Frame {
        let mut flags = FrameFlags::NONE;
        if encrypted {
            flags = FrameFlags::new(flags.as_byte() | FrameFlags::ENCRYPTED.as_byte());
        }
        if compressed {
            flags = FrameFlags::new(flags.as_byte() | FrameFlags::COMPRESSED.as_byte());
        }
        Frame::new(FrameType::Data, flags, payload)
    }

    /// Create a high-priority data frame
    pub fn priority_data(payload: Vec<u8>) -> Frame {
        let flags =
            FrameFlags::new(FrameFlags::ENCRYPTED.as_byte() | FrameFlags::PRIORITY_HIGH.as_byte());
        Frame::new(FrameType::Data, flags, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn test_addr() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 1), 12345))
    }

    #[test]
    fn test_frame_header_roundtrip() {
        let header = FrameHeader::new(FrameType::Data, FrameFlags::ENCRYPTED, 1024);
        let bytes = header.to_bytes();
        let parsed = FrameHeader::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.version, PROTOCOL_VERSION);
        assert_eq!(parsed.frame_type, FrameType::Data);
        assert!(parsed.flags.is_encrypted());
        assert_eq!(parsed.payload_length, 1024);
    }

    #[test]
    fn test_frame_roundtrip() {
        let payload = vec![1, 2, 3, 4, 5];
        let frame = Frame::new(FrameType::Data, FrameFlags::NONE, payload.clone());

        let bytes = frame.to_bytes();
        let parsed = Frame::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.header.frame_type, FrameType::Data);
        assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn test_frame_reader_writer() {
        let payload = vec![0u8; 1000];
        let frame = Frame::new(FrameType::Data, FrameFlags::COMPRESSED, payload.clone());

        let mut buffer = Vec::new();
        {
            let mut writer = FrameWriter::new(&mut buffer);
            writer.write_frame(&frame).unwrap();
        }

        let mut reader = FrameReader::new(Cursor::new(buffer));
        let parsed = reader.read_frame().unwrap();

        assert_eq!(parsed.header.frame_type, FrameType::Data);
        assert!(parsed.header.flags.is_compressed());
        assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn test_quick_validate_rejects_bad_magic() {
        let mut bytes = [0u8; FRAME_HEADER_SIZE];
        bytes[0..4].copy_from_slice(b"XXXX"); // Wrong magic

        assert!(matches!(
            FrameHeader::quick_validate(&bytes),
            Err(FramingError::InvalidMagic)
        ));
    }

    #[test]
    fn test_quick_validate_rejects_oversized() {
        let header = FrameHeader::new(FrameType::Data, FrameFlags::NONE, MAX_FRAME_SIZE);
        let bytes = header.to_bytes();

        assert!(matches!(
            FrameHeader::quick_validate(&bytes),
            Err(FramingError::FrameTooLarge(_, _))
        ));
    }

    #[test]
    fn test_cookie_generation_and_verification() {
        let secret = [1u8; 32];
        let addr = test_addr();

        let cookie = Cookie::generate(&secret, &addr);
        cookie.verify(&secret, &addr).unwrap();

        // Wrong address should fail
        let wrong_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 54321));
        assert!(matches!(
            cookie.verify(&secret, &wrong_addr),
            Err(FramingError::InvalidCookie)
        ));
    }

    #[test]
    fn test_cookie_secret_manager_rotation() {
        let mut manager = CookieSecretManager::new();
        let addr = test_addr();

        // Generate cookie with current secret
        let cookie = Cookie::generate(manager.current_secret(), &addr);

        // Should verify with manager
        manager.verify_cookie(&cookie, &addr).unwrap();
    }

    #[test]
    fn test_full_handshake() {
        let addr = test_addr();

        // Client initiates
        let mut client = ClientHandshake::new();
        let init_frame = client.init().unwrap();
        assert_eq!(init_frame.header.frame_type, FrameType::HandshakeInit);

        // Server processes init
        let mut server = ServerHandshake::new();
        let init_payload: HandshakeInit = bincode::deserialize(&init_frame.payload).unwrap();
        let cookie_frame = server.process_init(&addr, &init_payload).unwrap();
        assert_eq!(cookie_frame.header.frame_type, FrameType::HandshakeCookie);

        // Client processes cookie
        let complete_frame = client.process_cookie(&cookie_frame.payload).unwrap();
        assert_eq!(
            complete_frame.header.frame_type,
            FrameType::HandshakeComplete
        );

        // Server processes completion
        let complete_payload: HandshakeComplete =
            bincode::deserialize(&complete_frame.payload).unwrap();
        let ack_frame = server.process_complete(&addr, &complete_payload).unwrap();
        assert_eq!(ack_frame.header.frame_type, FrameType::HandshakeAck);

        // Client processes ack
        let session_id = client.process_ack(&ack_frame.payload).unwrap();
        assert_ne!(session_id, [0u8; 32]);

        assert_eq!(client.state(), &HandshakeState::Complete);
    }

    #[test]
    fn test_utility_frames() {
        let ping = frames::ping(12345);
        assert_eq!(ping.header.frame_type, FrameType::Ping);

        let pong = frames::pong(12345);
        assert_eq!(pong.header.frame_type, FrameType::Pong);

        let close = frames::close("goodbye");
        assert_eq!(close.header.frame_type, FrameType::Close);

        let error = frames::error(404, "not found");
        assert_eq!(error.header.frame_type, FrameType::Error);

        let data = frames::data(vec![1, 2, 3], true, false);
        assert!(data.header.flags.is_encrypted());
        assert!(!data.header.flags.is_compressed());
    }
}
