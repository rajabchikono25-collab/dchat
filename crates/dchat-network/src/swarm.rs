//! Network swarm management

use crate::{
    behavior::{DchatBehavior, DchatMessage},
    discovery::{Discovery, DiscoveryConfig},
    gossip::FloodControl,
    nat::{NatConfig, NatTraversal},
    routing::Router,
    transport::build_transport_with_relay,
};
use dchat_core::error::{Error, Result};
use futures::StreamExt;
use libp2p::multiaddr::Protocol;
use libp2p::{
    gossipsub, identify, kad, mdns, relay,
    swarm::{Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Network manager configuration
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Listen addresses
    pub listen_addrs: Vec<Multiaddr>,

    /// Discovery configuration
    pub discovery: DiscoveryConfig,

    /// NAT traversal configuration
    pub nat: NatConfig,

    /// External/public address to announce (bypasses NAT detection)
    /// Format: "/ip4/<public_ip>/tcp/<port>"
    pub external_address: Option<Multiaddr>,

    /// Network-level rate limiting and connection limits
    pub rate_limits: RateLimitConfig,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addrs: vec![
                "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
                "/ip6/::/tcp/0".parse().unwrap(),
            ],
            discovery: DiscoveryConfig::default(),
            nat: NatConfig::default(),
            external_address: None,
            rate_limits: RateLimitConfig::default(),
        }
    }
}

/// Rate limiting configuration for network-level protections
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Global message rate (messages/sec). 0 disables.
    pub max_messages_per_second: u32,
    /// Per-peer message rate (messages/sec). 0 disables.
    pub max_messages_per_peer_per_second: u32,
    /// Maximum concurrent peer connections. 0 disables.
    pub max_connections: usize,
    /// Maximum concurrent connections per IP. 0 disables.
    pub max_connections_per_ip: u32,
    /// Maximum aggregate inbound bandwidth per second (bytes). 0 disables.
    pub max_bandwidth_bytes_per_sec: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_messages_per_second: 0,
            max_messages_per_peer_per_second: 0,
            max_connections: 0,
            max_connections_per_ip: 0,
            max_bandwidth_bytes_per_sec: 0,
        }
    }
}

/// Network events
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// New peer discovered
    PeerDiscovered(PeerId),

    /// Peer connected with optional remote address
    PeerConnected {
        peer_id: PeerId,
        /// Remote address of the peer (if known)
        endpoint: Option<Multiaddr>,
    },

    /// Peer disconnected
    PeerDisconnected(PeerId),

    /// Message received
    MessageReceived { from: PeerId, message: DchatMessage },

    /// DHT query completed
    DhtQueryComplete,
}

/// Network manager
pub struct NetworkManager {
    swarm: Swarm<DchatBehavior>,
    config: NetworkConfig,
    discovery: Discovery,
    nat: NatTraversal,
    router: Router,

    pending_kad_bootstrap: bool,

    // Rate limiting state
    flood_control: FloodControl,
    rate_limits: RateLimitConfig,
    bandwidth_window: BandwidthWindow,

    // Connection accounting
    connected_peers: usize,
    connections_per_ip: HashMap<String, u32>,
    peer_ip_index: HashMap<PeerId, String>,
}

#[derive(Debug, Clone)]
struct BandwidthWindow {
    bytes: u64,
    window_start: Instant,
}

impl NetworkManager {
    /// Create a new network manager with a random identity
    pub async fn new(config: NetworkConfig) -> Result<Self> {
        Self::with_keypair(config, None).await
    }

    /// Create a new network manager with an optional keypair
    /// If keypair is None, a random one is generated
    pub async fn with_keypair(
        mut config: NetworkConfig,
        keypair: Option<libp2p::identity::Keypair>,
    ) -> Result<Self> {
        // Prepare rate limit configuration (0 values disable the guard)
        let rate_limits = config.rate_limits.clone();
        let per_peer_limit = if rate_limits.max_messages_per_peer_per_second == 0 {
            u32::MAX
        } else {
            rate_limits.max_messages_per_peer_per_second
        };
        let global_limit = if rate_limits.max_messages_per_second == 0 {
            u32::MAX
        } else {
            rate_limits.max_messages_per_second
        };

        // Use provided keypair or generate a random one
        let local_key = keypair.unwrap_or_else(libp2p::identity::Keypair::generate_ed25519);
        let local_peer_id = local_key.public().to_peer_id();

        // Keep DiscoveryConfig consistent with the swarm peer id.
        // This avoids internal discovery state tracking a different identity.
        config.discovery.local_peer_id = local_peer_id;

        tracing::info!("Local peer ID: {}", local_peer_id);

        // Create relay client - this returns (transport, behavior) pair
        // The transport must stay alive as long as the behavior is used
        let (relay_transport, relay_client) = relay::client::new(local_peer_id);

        // Build transport with relay support
        let transport = build_transport_with_relay(&local_key, relay_transport)?;

        // Create behavior with relay client
        let behavior = DchatBehavior::new(
            local_peer_id,
            &local_key,
            relay_client,
            config.discovery.enable_mdns,
        )
        .map_err(|e| Error::network(format!("Failed to create behavior: {}", e)))?;

        // Build swarm using new API
        let swarm_config = libp2p::swarm::Config::with_tokio_executor();
        let swarm = Swarm::new(transport, behavior, local_peer_id, swarm_config);

        let discovery = Discovery::new(config.discovery.clone()).await?;
        let nat = NatTraversal::new(config.nat.clone()).await?;
        let router = Router::new();
        let flood_control = FloodControl::new(per_peer_limit, global_limit);
        let bandwidth_window = BandwidthWindow {
            bytes: 0,
            window_start: Instant::now(),
        };

        Ok(Self {
            swarm,
            config,
            discovery,
            nat,
            router,
            pending_kad_bootstrap: false,
            flood_control,
            rate_limits,
            bandwidth_window,
            connected_peers: 0,
            connections_per_ip: HashMap::new(),
            peer_ip_index: HashMap::new(),
        })
    }

    fn ensure_p2p_addr(addr: &Multiaddr, peer_id: PeerId) -> Multiaddr {
        let mut out = addr.clone();
        let has_p2p = addr.iter().any(|p| matches!(p, Protocol::P2p(_)));
        if !has_p2p {
            out.push(Protocol::P2p(peer_id));
        }
        out
    }

    fn strip_p2p(addr: &Multiaddr) -> Multiaddr {
        let mut out = Multiaddr::empty();
        for p in addr.iter() {
            if matches!(p, Protocol::P2p(_)) {
                continue;
            }
            out.push(p);
        }
        out
    }

    fn extract_ip(addr: &Multiaddr) -> Option<String> {
        for proto in addr.iter() {
            match proto {
                Protocol::Ip4(ip) => return Some(ip.to_string()),
                Protocol::Ip6(ip) => return Some(ip.to_string()),
                _ => continue,
            }
        }
        None
    }

    fn allow_and_record_bandwidth(&mut self, bytes: u64) -> bool {
        if self.rate_limits.max_bandwidth_bytes_per_sec == 0 {
            return true;
        }

        let now = Instant::now();
        if now.duration_since(self.bandwidth_window.window_start) >= Duration::from_secs(1) {
            self.bandwidth_window.bytes = 0;
            self.bandwidth_window.window_start = now;
        }

        if self.bandwidth_window.bytes + bytes > self.rate_limits.max_bandwidth_bytes_per_sec {
            tracing::warn!(
                "⚠️ Bandwidth cap exceeded: {} bytes in current window (limit {} bytes)",
                self.bandwidth_window.bytes,
                self.rate_limits.max_bandwidth_bytes_per_sec
            );
            return false;
        }

        self.bandwidth_window.bytes += bytes;
        true
    }

    fn accept_connection(&mut self, peer_id: &PeerId, addr: Option<&Multiaddr>) -> bool {
        if self.rate_limits.max_connections > 0
            && self.connected_peers >= self.rate_limits.max_connections
        {
            tracing::warn!(
                "🚫 Rejecting peer {}: max connections reached ({})",
                peer_id,
                self.rate_limits.max_connections
            );
            return false;
        }

        if let Some(addr) = addr.and_then(Self::extract_ip) {
            let counter = self.connections_per_ip.entry(addr.clone()).or_insert(0);
            if self.rate_limits.max_connections_per_ip > 0
                && *counter >= self.rate_limits.max_connections_per_ip
            {
                tracing::warn!(
                    "🚫 Rejecting peer {} from {}: per-IP connection cap {} reached",
                    peer_id,
                    addr,
                    self.rate_limits.max_connections_per_ip
                );
                return false;
            }

            *counter += 1;
            self.peer_ip_index.insert(*peer_id, addr);
        }

        self.connected_peers += 1;
        true
    }

    fn on_disconnect(&mut self, peer_id: &PeerId) {
        if self.connected_peers > 0 {
            self.connected_peers -= 1;
        }

        if let Some(ip) = self.peer_ip_index.remove(peer_id) {
            if let Some(count) = self.connections_per_ip.get_mut(&ip) {
                if *count > 0 {
                    *count -= 1;
                }
                if *count == 0 {
                    self.connections_per_ip.remove(&ip);
                }
            }
        }
    }

    fn message_allowed(&mut self, peer_id: &PeerId, payload_len: u64) -> bool {
        if !self.allow_and_record_bandwidth(payload_len) {
            tracing::warn!(
                "Dropping message from {}: bandwidth limit exceeded ({} bytes)",
                peer_id,
                payload_len
            );
            return false;
        }

        let rate_limits_disabled = self.rate_limits.max_messages_per_second == 0
            && self.rate_limits.max_messages_per_peer_per_second == 0;

        if rate_limits_disabled {
            return true;
        }

        // Rate limit disabled if limits are zero (set to u32::MAX on init)
        if !self.flood_control.check_rate_limit(peer_id) {
            tracing::warn!("Dropping message from {}: rate limit exceeded", peer_id);
            return false;
        }

        self.flood_control.record_message(peer_id);
        true
    }

    fn try_kad_bootstrap(&mut self, reason: &'static str) {
        match self.swarm.behaviour_mut().kademlia.bootstrap() {
            Ok(query_id) => {
                tracing::info!(
                    "📡 DHT bootstrap started ({}, query={:?})",
                    reason,
                    query_id
                );
                self.pending_kad_bootstrap = false;
            }
            Err(e) => {
                // This commonly fails at startup when we haven't connected to any peer yet.
                // Keep running and retry once we establish a connection.
                tracing::warn!(
                    "⚠️ DHT bootstrap attempt failed ({}): {} (will retry after connecting)",
                    reason,
                    e
                );
                self.pending_kad_bootstrap = true;
            }
        }
    }

    /// Get the message router for direct routing operations
    pub fn router(&self) -> &Router {
        &self.router
    }

    /// Start the network manager
    pub async fn start(&mut self) -> Result<()> {
        // Check if external address is manually configured (bypasses NAT detection)
        if let Some(ref external_addr) = self.config.external_address {
            tracing::info!(
                "📡 Using manually configured external address: {}",
                external_addr
            );
            self.swarm.add_external_address(external_addr.clone());
        } else {
            // PRODUCTION: Perform NAT detection and establish connectivity
            tracing::info!("🔍 Detecting NAT type and external address...");

            match self.nat.detect().await {
                Ok((nat_type, external_addr)) => {
                    tracing::info!("✓ NAT detection complete");
                    tracing::info!("  NAT Type: {:?}", nat_type);
                    if let Some(addr) = external_addr {
                        tracing::info!("  External Address: {}", addr);
                    }

                    // Establish connectivity using best strategy for detected NAT type
                    let local_port = self
                        .config
                        .listen_addrs
                        .first()
                        .and_then(|addr| {
                            addr.iter().find_map(|proto| {
                                if let libp2p::multiaddr::Protocol::Tcp(port) = proto {
                                    Some(port)
                                } else {
                                    None
                                }
                            })
                        })
                        .unwrap_or(0);

                    match self.nat.establish_connectivity(local_port).await {
                        Ok(connectivity) => {
                            tracing::info!("✅ NAT traversal successful!");
                            tracing::info!("  Method: {:?}", connectivity.method);
                            tracing::info!("  External: {}", connectivity.external_addr);
                            tracing::info!("  Local: {}", connectivity.local_addr);

                            // Add external address to swarm for advertising
                            let external_multiaddr = format!(
                                "/ip4/{}/tcp/{}",
                                connectivity.external_addr.ip(),
                                connectivity.external_addr.port()
                            )
                            .parse::<Multiaddr>()
                            .map_err(|e| {
                                Error::network(format!("Invalid external address: {}", e))
                            })?;

                            self.swarm.add_external_address(external_multiaddr);
                        }
                        Err(e) => {
                            tracing::warn!("⚠️ NAT traversal failed: {}", e);
                            tracing::warn!("  Continuing with local connectivity only");
                            tracing::warn!(
                                "  This node may not be reachable from outside the local network"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("⚠️ NAT detection failed: {}", e);
                    tracing::warn!("  Continuing without NAT traversal");
                }
            }
        }

        // Listen on configured addresses
        for addr in &self.config.listen_addrs {
            self.swarm
                .listen_on(addr.clone())
                .map_err(|e| Error::network(format!("Failed to listen: {}", e)))?;
        }

        // Bootstrap DHT with known peers
        let bootstrap_nodes = self.discovery.bootstrap_nodes();
        if !bootstrap_nodes.is_empty() {
            tracing::info!(
                "📡 Configuring {} bootstrap peers from config",
                bootstrap_nodes.len()
            );

            for (peer_id, addr) in bootstrap_nodes {
                let peer_id = *peer_id;
                let addr = addr.clone();

                let dial_addr = Self::ensure_p2p_addr(&addr, peer_id);
                let kad_addr = Self::strip_p2p(&dial_addr);

                tracing::info!("  → Bootstrap peer: {} at {}", peer_id, dial_addr);
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, kad_addr);

                // Actively dial each bootstrap peer, enforcing the expected PeerId.
                match self.swarm.dial(dial_addr) {
                    Ok(_) => tracing::debug!("Dialing bootstrap peer: {}", peer_id),
                    Err(e) => tracing::warn!("Failed to dial bootstrap peer {}: {}", peer_id, e),
                }
            }

            // Trigger the DHT bootstrap. If we haven't connected yet, we'll retry later.
            self.try_kad_bootstrap("startup");
        } else if self.config.discovery.enable_mdns {
            // No bootstrap nodes configured - will rely on mDNS for local discovery
            // and wait for other peers to connect
            tracing::info!(
                "No bootstrap nodes configured - will use mDNS for local peer discovery"
            );
        } else {
            // With both bootstrap and mDNS disabled, the node will likely remain isolated.
            tracing::warn!(
                "No bootstrap nodes configured and mDNS is disabled; node may remain isolated"
            );
        }

        tracing::info!(
            "Network started, listening on {} addresses",
            self.config.listen_addrs.len()
        );

        Ok(())
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Get local peer ID (alias for compatibility)
    pub fn peer_id(&self) -> PeerId {
        self.local_peer_id()
    }

    /// Get listen addresses
    pub fn listeners(&self) -> Vec<Multiaddr> {
        self.swarm.listeners().cloned().collect()
    }

    /// Dial a peer
    pub fn dial(&mut self, addr: Multiaddr) -> Result<()> {
        self.swarm
            .dial(addr)
            .map_err(|e| Error::network(format!("Failed to dial: {}", e)))
    }

    /// Subscribe to a channel
    pub fn subscribe_channel(&mut self, channel_id: &str) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .subscribe_channel(channel_id)
            .map_err(|e| Error::network(format!("Subscribe failed: {}", e)))?;
        Ok(())
    }

    /// Subscribe to a channel topic
    pub fn subscribe_to_channel(&mut self, channel_id: &str) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .subscribe_channel(channel_id)
            .map_err(|e| Error::network(format!("Subscribe failed: {}", e)))?;
        tracing::info!("📢 Subscribed to channel: {}", channel_id);
        Ok(())
    }

    /// Unsubscribe from a gossipsub channel topic
    pub fn unsubscribe_from_channel(&mut self, channel_id: &str) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .unsubscribe_channel(channel_id)
            .map_err(|e| Error::network(format!("Unsubscribe failed: {}", e)))?;
        tracing::info!("🔕 Unsubscribed from channel: {}", channel_id);
        Ok(())
    }

    /// Publish message to channel
    pub fn publish_to_channel(&mut self, channel_id: &str, message: &DchatMessage) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .publish_to_channel(channel_id, message)
            .map_err(|e| Error::network(format!("Publish failed: {}", e)))?;
        Ok(())
    }

    /// Subscribe to validator consensus topic
    pub fn subscribe_validators(&mut self) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .subscribe_validators()
            .map_err(|e| Error::network(format!("Validator subscribe failed: {}", e)))?;
        tracing::info!("📢 Subscribed to validator consensus network");
        Ok(())
    }

    /// Broadcast validator block to consensus network
    pub fn broadcast_validator_block(&mut self, message: &DchatMessage) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .broadcast_validator_block(message)
            .map_err(|e| Error::network(format!("Validator broadcast failed: {}", e)))?;
        Ok(())
    }

    /// Get gossipsub mesh peer count for debugging
    pub fn get_mesh_peer_count(&mut self, channel_id: &str) -> usize {
        let topic_hash = gossipsub::IdentTopic::new(format!("dchat/channel/{}", channel_id)).hash();
        self.swarm
            .behaviour_mut()
            .gossipsub
            .mesh_peers(&topic_hash)
            .count()
    }

    /// Get all mesh peers for a channel
    pub fn get_mesh_peers(&mut self, channel_id: &str) -> Vec<PeerId> {
        let topic_hash = gossipsub::IdentTopic::new(format!("dchat/channel/{}", channel_id)).hash();
        self.swarm
            .behaviour_mut()
            .gossipsub
            .mesh_peers(&topic_hash)
            .copied()
            .collect()
    }

    /// Send a handshake to a peer
    pub fn send_handshake(&mut self, peer_id: PeerId, handshake_data: Vec<u8>) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .send_handshake(peer_id, handshake_data);
        tracing::debug!("Sent handshake to peer: {}", peer_id);
        Ok(())
    }

    /// Disconnect from a peer
    ///
    /// Closes all connections to the specified peer.
    pub fn disconnect_peer(&mut self, peer_id: &PeerId) -> Result<()> {
        let _ = self.swarm.disconnect_peer_id(*peer_id);
        self.discovery.peer_disconnected(peer_id);
        tracing::info!("🔌 Disconnected from peer: {}", peer_id);
        Ok(())
    }

    /// Process network events
    pub async fn next_event(&mut self) -> Option<NetworkEvent> {
        loop {
            let event = self.swarm.select_next_some().await;

            match event {
                SwarmEvent::Behaviour(event) => {
                    if let Some(net_event) = self.handle_behavior_event(event) {
                        return Some(net_event);
                    }
                }
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    let remote_addr = endpoint.get_remote_address().clone();
                    if !self.accept_connection(&peer_id, Some(&remote_addr)) {
                        let _ = self.swarm.disconnect_peer_id(peer_id);
                        continue;
                    }

                    tracing::info!(
                        "🔗 Connection established with peer: {} via {:?}",
                        peer_id,
                        endpoint
                    );
                    self.discovery.peer_connected(peer_id);

                    if self.pending_kad_bootstrap {
                        self.try_kad_bootstrap("post-connect");
                    }

                    return Some(NetworkEvent::PeerConnected {
                        peer_id,
                        endpoint: Some(remote_addr),
                    });
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    tracing::info!("🔌 Connection closed with peer: {}", peer_id);
                    self.on_disconnect(&peer_id);
                    self.discovery.peer_disconnected(&peer_id);
                    return Some(NetworkEvent::PeerDisconnected(peer_id));
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!("Listening on: {}", address);
                }
                _ => {}
            }
        }
    }

    fn handle_behavior_event(
        &mut self,
        event: crate::behavior::DchatBehaviorEvent,
    ) -> Option<NetworkEvent> {
        use crate::behavior::DchatBehaviorEvent;
        use crate::behavior::HandshakeData;

        match event {
            DchatBehaviorEvent::Mdns(mdns::Event::Discovered(peers)) => {
                for (peer_id, addr) in peers {
                    tracing::info!("✨ mDNS discovered peer: {} at {}", peer_id, addr);
                    // Register peer with default address
                    let peer_info = crate::discovery::PeerInfo::new(peer_id, vec![]);
                    let _ = self.discovery.register_peer(peer_info);
                    let dial_addr = Self::ensure_p2p_addr(&addr, peer_id);
                    let kad_addr = Self::strip_p2p(&dial_addr);
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, kad_addr);

                    // Attempt to dial discovered peers immediately.
                    if let Err(e) = self.swarm.dial(dial_addr) {
                        tracing::debug!("mDNS dial failed for {}: {}", peer_id, e);
                    }
                }
                None
            }
            DchatBehaviorEvent::Mdns(mdns::Event::Expired(peers)) => {
                for (peer_id, _) in peers {
                    tracing::debug!("mDNS peer expired: {}", peer_id);
                }
                None
            }
            DchatBehaviorEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message,
                ..
            }) => {
                if !self.message_allowed(&propagation_source, message.data.len() as u64) {
                    return None;
                }

                match crate::behavior::decode_wire_message(&message.data) {
                    Ok(dchat_msg) => Some(NetworkEvent::MessageReceived {
                        // Never fabricate identities. Use the peer that forwarded the message.
                        from: propagation_source,
                        message: dchat_msg,
                    }),
                    Err(e) => {
                        tracing::warn!(
                            "Dropping undecodable gossipsub message ({} bytes): {}",
                            message.data.len(),
                            e
                        );
                        None
                    }
                }
            }
            DchatBehaviorEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic }) => {
                tracing::info!("🔔 Peer {} subscribed to topic: {}", peer_id, topic);
                None
            }
            DchatBehaviorEvent::Gossipsub(gossipsub::Event::Unsubscribed { peer_id, topic }) => {
                tracing::info!("🔕 Peer {} unsubscribed from topic: {}", peer_id, topic);
                None
            }
            DchatBehaviorEvent::Gossipsub(gossipsub::Event::GossipsubNotSupported { peer_id }) => {
                tracing::warn!("⚠️  Peer {} does not support gossipsub", peer_id);
                None
            }
            DchatBehaviorEvent::Identify(identify::Event::Received {
                peer_id,
                info,
                connection_id: _,
            }) => {
                tracing::info!(
                    "Identified peer: {} with {} addresses",
                    peer_id,
                    info.listen_addrs.len()
                );
                for addr in info.listen_addrs {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr);
                }
                None
            }
            DchatBehaviorEvent::Kademlia(kad::Event::OutboundQueryProgressed {
                result: kad::QueryResult::Bootstrap(Ok(_)),
                ..
            }) => {
                tracing::info!("DHT bootstrap successful");
                Some(NetworkEvent::DhtQueryComplete)
            }
            DchatBehaviorEvent::ReqResp(event) => {
                use libp2p_request_response::{Event as ReqRespEvent, Message as ReqRespMessage};

                match event {
                    ReqRespEvent::Message { peer, message } => match message {
                        ReqRespMessage::Request {
                            request,
                            channel,
                            request_id: _,
                        } => {
                            if !self.message_allowed(&peer, request.data.len() as u64) {
                                return None;
                            }

                            // Always respond so the remote peer doesn't time out.
                            let response = HandshakeData { data: Vec::new() };
                            if let Err(e) = self
                                .swarm
                                .behaviour_mut()
                                .req_resp
                                .send_response(channel, response)
                            {
                                tracing::warn!(
                                    "Failed to send req-resp response to {}: {:?}",
                                    peer,
                                    e
                                );
                            }

                            match crate::behavior::decode_wire_message(&request.data) {
                                Ok(dchat_msg) => Some(NetworkEvent::MessageReceived {
                                    from: peer,
                                    message: dchat_msg,
                                }),
                                Err(e) => {
                                    tracing::warn!(
                                        "Dropping undecodable req-resp message from {} ({} bytes): {}",
                                        peer,
                                        request.data.len(),
                                        e
                                    );
                                    None
                                }
                            }
                        }
                        ReqRespMessage::Response {
                            response,
                            request_id: _,
                        } => {
                            // Currently used as an ack channel; log only if response contains data.
                            if !response.data.is_empty() {
                                tracing::debug!(
                                    "Received req-resp response from {} ({} bytes)",
                                    peer,
                                    response.data.len()
                                );
                            }
                            None
                        }
                    },
                    ReqRespEvent::OutboundFailure {
                        peer,
                        error,
                        request_id: _,
                    } => {
                        tracing::warn!("Req-resp outbound failure to {}: {}", peer, error);
                        None
                    }
                    ReqRespEvent::InboundFailure {
                        peer,
                        error,
                        request_id: _,
                    } => {
                        tracing::warn!("Req-resp inbound failure from {}: {}", peer, error);
                        None
                    }
                    ReqRespEvent::ResponseSent {
                        peer,
                        request_id: _,
                    } => {
                        tracing::trace!("Req-resp response sent to {}", peer);
                        None
                    }
                }
            }
            _ => None,
        }
    }

    /// Shutdown network manager and cleanup resources
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("🛑 Shutting down network manager...");

        // Cleanup NAT traversal resources (UPnP mappings, TURN relays)
        if let Err(e) = self.nat.shutdown().await {
            tracing::warn!("NAT cleanup failed: {}", e);
        } else {
            tracing::info!("✓ NAT resources cleaned up");
        }

        // Close all swarm connections
        tracing::info!("✓ Network manager shutdown complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_manager_creation() {
        let config = NetworkConfig::default();
        let manager = NetworkManager::new(config).await;
        assert!(manager.is_ok());
    }
}
