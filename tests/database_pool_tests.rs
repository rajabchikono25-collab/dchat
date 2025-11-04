//! Integration tests for database connection pooling

use dchat_storage::database::{Database, DatabaseConfig};
use tempfile::tempdir;

#[tokio::test]
async fn test_connection_pool_creation() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let config = DatabaseConfig {
        path: db_path,
        max_connections: 5,
        connection_timeout_secs: 10,
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true,
    };

    let db = Database::new(config)
        .await
        .expect("Failed to create database");

    // Verify pool metrics
    let metrics = db.pool_metrics();
    assert!(metrics.size > 0);
    assert_eq!(metrics.max_connections, 5);
}

#[tokio::test]
async fn test_health_check() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let config = DatabaseConfig {
        path: db_path,
        max_connections: 3,
        connection_timeout_secs: 10,
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true,
    };

    let db = Database::new(config)
        .await
        .expect("Failed to create database");

    // Perform health check
    let health = db.health_check().await.expect("Health check failed");
    assert!(health.is_healthy);
    assert!(health.pool_size > 0);
    assert!(health.acquire_time_ms < 1000); // Should be fast
    assert!(health.query_time_ms < 1000);
}

#[tokio::test]
async fn test_connection_validation() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let config = DatabaseConfig {
        path: db_path,
        max_connections: 4,
        connection_timeout_secs: 10,
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true,
    };

    let db = Database::new(config)
        .await
        .expect("Failed to create database");

    // Validate connections
    let validation = db.validate_connections().await.expect("Validation failed");
    assert!(validation.valid_connections > 0);
    assert_eq!(validation.invalid_connections, 0);
    assert!(validation.validation_passed);
    assert_eq!(validation.max_connections, 4);
}

#[tokio::test]
async fn test_concurrent_database_operations() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let config = DatabaseConfig {
        path: db_path,
        max_connections: 10,
        connection_timeout_secs: 30,
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true,
    };

    let db = Database::new(config)
        .await
        .expect("Failed to create database");

    // Insert test user
    db.insert_user("user1", "alice", b"pubkey123")
        .await
        .expect("Failed to insert user");

    // Spawn concurrent read operations
    let mut handles = vec![];
    for i in 0..20 {
        let db_clone = db.clone();
        let handle = tokio::spawn(async move {
            if i % 2 == 0 {
                // Read operations
                db_clone
                    .get_user("user1")
                    .await
                    .expect("Failed to get user");
            } else {
                // Stats operations
                db_clone.stats().await.expect("Failed to get stats");
            }
        });
        handles.push(handle);
    }

    // Wait for all operations to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Verify pool is still healthy
    let health = db
        .health_check()
        .await
        .expect("Health check failed after concurrent ops");
    assert!(health.is_healthy);
}

#[tokio::test]
async fn test_pool_metrics_tracking() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let config = DatabaseConfig {
        path: db_path,
        max_connections: 8,
        connection_timeout_secs: 10,
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true,
    };

    let db = Database::new(config)
        .await
        .expect("Failed to create database");

    // Get initial metrics
    let metrics = db.pool_metrics();
    assert_eq!(metrics.max_connections, 8);

    // Perform some operations
    db.insert_user("user2", "bob", b"pubkey456")
        .await
        .expect("Failed to insert user");

    let _user = db.get_user("user2").await.expect("Failed to get user");

    // Get metrics again
    let metrics_after = db.pool_metrics();
    assert_eq!(metrics_after.max_connections, 8);
    assert!(metrics_after.size <= 8);
}

#[tokio::test]
async fn test_graceful_pool_close() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let config = DatabaseConfig {
        path: db_path,
        max_connections: 5,
        connection_timeout_secs: 10,
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true,
    };

    let db = Database::new(config)
        .await
        .expect("Failed to create database");

    // Perform operations
    db.insert_user("user3", "charlie", b"pubkey789")
        .await
        .expect("Failed to insert user");

    // Close the pool gracefully
    db.close().await.expect("Failed to close database");

    // After close, the pool should be closed
    // (We can't test this directly without trying to use a closed pool)
}

#[tokio::test]
async fn test_database_with_wal_mode() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test_wal.db");

    let config = DatabaseConfig {
        path: db_path,
        max_connections: 5,
        connection_timeout_secs: 10,
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true, // Enable WAL mode
    };

    let db = Database::new(config)
        .await
        .expect("Failed to create database");

    // Verify database is functional with WAL mode
    db.insert_user("user4", "david", b"pubkey012")
        .await
        .expect("Failed to insert user");

    let user = db.get_user("user4").await.expect("Failed to get user");
    assert!(user.is_some());
    assert_eq!(user.unwrap().username, "david");
}

#[tokio::test]
async fn test_pool_timeout_handling() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let config = DatabaseConfig {
        path: db_path,
        max_connections: 2,         // Very small pool
        connection_timeout_secs: 5, // Short timeout
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true,
    };

    let db = Database::new(config)
        .await
        .expect("Failed to create database");

    // Health check should succeed even with small pool
    let health = db.health_check().await.expect("Health check failed");
    assert!(health.is_healthy);
    assert!(health.acquire_time_ms < 5000); // Should be under timeout
}
