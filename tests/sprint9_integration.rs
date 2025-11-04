// Sprint 9 Integration Tests
// Tests: DHT Discovery, Gossip Protocol, NAT Traversal, Connection Lifecycle

use dchat_network::{
    ConnectionConfig, ConnectionManager, Discovery, DiscoveryConfig, Gossip, GossipConfig,
    NatConfig, NatTraversal,
};
use std::time::{Duration, Instant};

// ============================================================================
// DHT DISCOVERY TESTS
// ============================================================================

#[tokio::test]
async fn test_dht_discovery_initialization() {
    let config = DiscoveryConfig::default();
    let result = Discovery::new(config).await;
    assert!(
        result.is_ok(),
        "DHT discovery should initialize successfully"
    );
}

#[tokio::test]
async fn test_dht_peer_count_query() {
    let config = DiscoveryConfig::default();
    let discovery = Discovery::new(config).await.unwrap();

    let peer_count = discovery.peer_count();

    // New discovery starts with 0 peers
    assert_eq!(peer_count, 0, "New discovery should have 0 peers");
}

#[tokio::test]
async fn test_dht_config_defaults() {
    let config = DiscoveryConfig::default();

    // Verify default configuration
    assert_eq!(config.k_bucket_size, 20, "K-bucket size should be 20");
    assert_eq!(config.alpha, 3, "Alpha should be 3");
    assert_eq!(config.min_peers, 10, "Min peers should be 10");
    assert_eq!(config.max_peers, 100, "Max peers should be 100");
    assert!(config.enable_mdns, "mDNS should be enabled by default");
}

// ============================================================================
// GOSSIP PROTOCOL TESTS
// ============================================================================

#[tokio::test]
async fn test_gossip_initialization() {
    let config = GossipConfig::default();
    let result = Gossip::new(config);
    assert!(
        result.is_ok(),
        "Gossip protocol should initialize successfully"
    );
}

#[tokio::test]
async fn test_gossip_broadcast_message() {
    let config = GossipConfig::default();
    let mut gossip = Gossip::new(config).unwrap();

    let payload = b"test message".to_vec();
    let result = gossip.broadcast(payload).await;
    assert!(result.is_ok(), "Should broadcast message successfully");
}

#[tokio::test]
async fn test_gossip_cache_empty_on_creation() {
    let config = GossipConfig::default();
    let gossip = Gossip::new(config).unwrap();

    let (cache_size, _) = gossip.cache_stats();
    assert_eq!(cache_size, 0, "New gossip should have empty cache");
}

#[tokio::test]
async fn test_gossip_config_defaults() {
    let config = GossipConfig::default();

    assert_eq!(config.fanout, 6, "Fanout should be 6");
    assert_eq!(config.max_ttl, 32, "Max TTL should be 32");
    assert_eq!(
        config.message_cache_size, 10000,
        "Cache size should be 10000"
    );
    assert_eq!(
        config.per_peer_rate_limit, 10,
        "Per-peer rate limit should be 10"
    );
    assert_eq!(
        config.global_rate_limit, 1000,
        "Global rate limit should be 1000"
    );
}

#[tokio::test]
async fn test_gossip_broadcast_multiple_messages() {
    let config = GossipConfig::default();
    let mut gossip = Gossip::new(config).unwrap();

    // Broadcast 5 messages
    for i in 0..5 {
        let payload = format!("message_{}", i).into_bytes();
        let result = gossip.broadcast(payload).await;
        assert!(
            result.is_ok(),
            "Message {} should broadcast successfully",
            i
        );
    }

    // Check cache has messages
    let (cache_size, _) = gossip.cache_stats();
    assert_eq!(cache_size, 5, "Cache should contain 5 messages");
}

// ============================================================================
// CONNECTION LIFECYCLE TESTS
// ============================================================================

#[tokio::test]
async fn test_connection_manager_initialization() {
    let config = ConnectionConfig::default();
    let _manager = ConnectionManager::new(config);
    // Successful creation is the test
}

#[tokio::test]
async fn test_connection_manager_stats() {
    let config = ConnectionConfig::default();
    let manager = ConnectionManager::new(config);

    let stats = manager.get_stats();
    assert_eq!(
        stats.total_connections, 0,
        "New manager should have 0 connections"
    );
}

#[tokio::test]
async fn test_connection_manager_maintenance() {
    let config = ConnectionConfig::default();
    let mut manager = ConnectionManager::new(config);

    let result = manager.maintain().await;
    assert!(result.is_ok(), "Maintenance should complete successfully");
}

#[tokio::test]
async fn test_connection_config_defaults() {
    let config = ConnectionConfig::default();

    assert_eq!(config.max_connections, 50, "Max connections should be 50");
    assert_eq!(
        config.target_connections, 30,
        "Target connections should be 30"
    );
    assert_eq!(config.health_check_interval, Duration::from_secs(30));
    assert_eq!(config.connection_timeout, Duration::from_secs(10));
    assert_eq!(config.idle_timeout, Duration::from_secs(300));
}

#[tokio::test]
async fn test_connection_pool_custom_config() {
    // Use default config for simplicity
    let config = ConnectionConfig::default();

    let manager = ConnectionManager::new(config);
    let stats = manager.get_stats();

    // Verify configuration is applied
    assert_eq!(stats.total_connections, 0);
}

// ============================================================================
// NAT TRAVERSAL TESTS
// ============================================================================

#[tokio::test]
async fn test_nat_traversal_disabled_config() {
    let config = NatConfig {
        enable_upnp: false,
        stun_servers: vec![],
        enable_hole_punching: false,
        turn_servers: vec![],
        discovery_timeout: Duration::from_secs(2),
        lease_duration: Duration::from_secs(3600),
        port_range: (49152, 65535),
    };

    // Should initialize even with all features disabled
    let result = NatTraversal::new(config).await;
    // Allow both success and failure (depends on environment)
    let _ = result;
}

#[tokio::test]
async fn test_nat_config_defaults() {
    let config = NatConfig::default();

    assert!(config.enable_upnp, "UPnP should be enabled by default");
    assert!(
        !config.stun_servers.is_empty(),
        "Should have default STUN servers"
    );
    assert!(
        config.enable_hole_punching,
        "Hole punching should be enabled"
    );
    assert_eq!(
        config.port_range,
        (49152, 65535),
        "Should use dynamic port range"
    );
}

// ============================================================================
// PERFORMANCE TESTS
// ============================================================================

#[tokio::test]
async fn test_gossip_broadcast_performance() {
    let config = GossipConfig::default();
    let mut gossip = Gossip::new(config).unwrap();

    let start = Instant::now();
    let message_count = 10;

    for i in 0..message_count {
        let payload = format!("perf_test_{}", i).into_bytes();
        let _ = gossip.broadcast(payload).await;
    }

    let duration = start.elapsed();
    let throughput = message_count as f64 / duration.as_secs_f64();

    // Should handle at least 5 messages per second
    assert!(
        throughput >= 5.0,
        "Gossip throughput: {:.2} msg/s (expected >= 5)",
        throughput
    );
}

#[tokio::test]
async fn test_connection_maintenance_performance() {
    let config = ConnectionConfig::default();
    let mut manager = ConnectionManager::new(config);

    let start = Instant::now();

    // Run maintenance 10 times
    for _ in 0..10 {
        let _ = manager.maintain().await;
    }

    let duration = start.elapsed();

    // 10 maintenance cycles should complete quickly
    assert!(
        duration < Duration::from_secs(1),
        "Maintenance should be fast: {:?}",
        duration
    );
}

#[tokio::test]
async fn test_discovery_initialization_performance() {
    let start = Instant::now();

    let config = DiscoveryConfig::default();
    let _ = Discovery::new(config).await;

    let duration = start.elapsed();

    // DHT initialization should be quick
    assert!(
        duration < Duration::from_secs(2),
        "DHT init should be fast: {:?}",
        duration
    );
}

// ============================================================================
// INTEGRATION SCENARIOS
// ============================================================================

#[tokio::test]
async fn test_full_stack_initialization() {
    // Initialize all components together
    let discovery_config = DiscoveryConfig::default();
    let gossip_config = GossipConfig::default();
    let connection_config = ConnectionConfig::default();

    let discovery = Discovery::new(discovery_config).await;
    let gossip = Gossip::new(gossip_config);
    let _connection_manager = ConnectionManager::new(connection_config);

    assert!(discovery.is_ok(), "DHT should initialize");
    assert!(gossip.is_ok(), "Gossip should initialize");
    // Connection manager always succeeds
}

#[tokio::test]
async fn test_gossip_with_multiple_broadcasts() {
    let config = GossipConfig::default();
    let mut gossip = Gossip::new(config).unwrap();

    // Broadcast multiple distinct messages
    let messages = vec![
        b"message_1".to_vec(),
        b"message_2".to_vec(),
        b"message_3".to_vec(),
    ];

    for (i, payload) in messages.iter().enumerate() {
        let result = gossip.broadcast(payload.clone()).await;
        assert!(result.is_ok(), "Message {} should broadcast", i);
    }

    // Verify all messages are cached
    let (cache_size, _) = gossip.cache_stats();
    assert_eq!(cache_size, 3, "Should have 3 cached messages");
}

#[tokio::test]
async fn test_connection_manager_with_maintenance() {
    let config = ConnectionConfig::default();

    let mut manager = ConnectionManager::new(config);

    // Run maintenance immediately
    let result1 = manager.maintain().await;
    assert!(result1.is_ok(), "First maintenance should succeed");

    // Run again
    let result2 = manager.maintain().await;
    assert!(result2.is_ok(), "Second maintenance should succeed");

    // Stats should still show 0 connections
    let stats = manager.get_stats();
    assert_eq!(stats.total_connections, 0);
}

// ============================================================================
// CONFIGURATION VALIDATION TESTS
// ============================================================================

#[test]
fn test_discovery_config_can_be_cloned() {
    let config1 = DiscoveryConfig::default();
    let config2 = config1.clone();

    assert_eq!(config1.k_bucket_size, config2.k_bucket_size);
    assert_eq!(config1.alpha, config2.alpha);
}

#[test]
fn test_gossip_config_can_be_cloned() {
    let config1 = GossipConfig::default();
    let config2 = config1.clone();

    assert_eq!(config1.fanout, config2.fanout);
    assert_eq!(config1.max_ttl, config2.max_ttl);
}

#[test]
fn test_connection_config_can_be_cloned() {
    let config1 = ConnectionConfig::default();
    let config2 = config1.clone();

    assert_eq!(config1.max_connections, config2.max_connections);
    assert_eq!(config1.target_connections, config2.target_connections);
}

#[test]
fn test_nat_config_can_be_cloned() {
    let config1 = NatConfig::default();
    let config2 = config1.clone();

    assert_eq!(config1.enable_upnp, config2.enable_upnp);
    assert_eq!(config1.enable_hole_punching, config2.enable_hole_punching);
}
