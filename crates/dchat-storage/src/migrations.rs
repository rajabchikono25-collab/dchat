//! Database migration runner for storage optimization
//!
//! This module provides automated migration execution for the storage optimization system.
//! Migrations are applied in order and tracked in the database to prevent re-application.

use sqlx::{PgPool, Postgres, Transaction};
use tracing::{error, info, warn};

/// Migration metadata
#[derive(Debug, Clone)]
pub struct Migration {
    pub id: &'static str,
    pub name: &'static str,
    pub sql: &'static str,
}

/// List of all migrations in order
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        id: "20251103_001",
        name: "create_content_store",
        sql: include_str!("../migrations/20251103_001_create_content_store.sql"),
    },
    Migration {
        id: "20251103_002",
        name: "create_storage_bonds",
        sql: include_str!("../migrations/20251103_002_create_storage_bonds.sql"),
    },
    Migration {
        id: "20251103_003",
        name: "create_micropayment_streams",
        sql: include_str!("../migrations/20251103_003_create_micropayment_streams.sql"),
    },
    Migration {
        id: "20251103_004",
        name: "add_messages_tier_columns",
        sql: include_str!("../migrations/20251103_004_add_messages_tier_columns.sql"),
    },
    Migration {
        id: "20251103_005",
        name: "create_analytics_views",
        sql: include_str!("../migrations/20251103_005_create_analytics_views.sql"),
    },
    Migration {
        id: "20251210_001",
        name: "storage_provider_registry",
        sql: include_str!("../migrations/20251210_001_storage_provider_registry.sql"),
    },
];

/// Migration runner
pub struct MigrationRunner {
    pool: PgPool,
}

impl MigrationRunner {
    /// Create a new migration runner
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Initialize the migrations tracking table
    async fn init_migrations_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS _schema_migrations (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!("Migrations tracking table initialized");
        Ok(())
    }

    /// Check if a migration has been applied
    async fn is_applied(&self, migration_id: &str) -> Result<bool, sqlx::Error> {
        let result: Option<(String,)> =
            sqlx::query_as("SELECT id FROM _schema_migrations WHERE id = $1")
                .bind(migration_id)
                .fetch_optional(&self.pool)
                .await?;

        Ok(result.is_some())
    }

    /// Mark a migration as applied
    async fn mark_applied(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        migration: &Migration,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO _schema_migrations (id, name) VALUES ($1, $2)")
            .bind(migration.id)
            .bind(migration.name)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }

    /// Run a single migration
    async fn run_migration(&self, migration: &Migration) -> Result<(), sqlx::Error> {
        // Check if already applied
        if self.is_applied(migration.id).await? {
            info!(
                migration_id = migration.id,
                migration_name = migration.name,
                "Migration already applied, skipping"
            );
            return Ok(());
        }

        info!(
            migration_id = migration.id,
            migration_name = migration.name,
            "Applying migration"
        );

        // Begin transaction
        let mut tx = self.pool.begin().await?;

        // Execute migration SQL
        sqlx::query(migration.sql)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                error!(
                    migration_id = migration.id,
                    error = ?e,
                    "Migration failed"
                );
                e
            })?;

        // Mark as applied
        self.mark_applied(&mut tx, migration).await?;

        // Commit transaction
        tx.commit().await?;

        info!(
            migration_id = migration.id,
            migration_name = migration.name,
            "Migration applied successfully"
        );

        Ok(())
    }

    /// Run all pending migrations
    pub async fn run_all(&self) -> Result<usize, sqlx::Error> {
        // Initialize migrations table
        self.init_migrations_table().await?;

        let mut applied_count = 0;

        // Run each migration in order
        for migration in MIGRATIONS {
            if !self.is_applied(migration.id).await? {
                self.run_migration(migration).await?;
                applied_count += 1;
            }
        }

        if applied_count == 0 {
            info!("All migrations already applied");
        } else {
            info!(
                applied_count = applied_count,
                "Migrations completed successfully"
            );
        }

        Ok(applied_count)
    }

    /// Get list of applied migrations
    pub async fn list_applied(
        &self,
    ) -> Result<Vec<(String, String, chrono::DateTime<chrono::Utc>)>, sqlx::Error> {
        // Ensure table exists
        self.init_migrations_table().await?;

        let results: Vec<(String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            "SELECT id, name, applied_at FROM _schema_migrations ORDER BY applied_at",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }

    /// Get list of pending migrations
    pub async fn list_pending(&self) -> Result<Vec<&Migration>, sqlx::Error> {
        let mut pending = Vec::new();

        for migration in MIGRATIONS {
            if !self.is_applied(migration.id).await? {
                pending.push(migration);
            }
        }

        Ok(pending)
    }

    /// Verify database schema matches expected state
    pub async fn verify(&self) -> Result<bool, sqlx::Error> {
        // Check that all tables exist
        let tables = vec!["content_store", "storage_bonds", "micropayment_streams"];

        for table in &tables {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                    SELECT FROM information_schema.tables 
                    WHERE table_schema = 'public' 
                    AND table_name = $1
                )",
            )
            .bind(table)
            .fetch_one(&self.pool)
            .await?;

            if !exists {
                warn!(table = table, "Required table does not exist");
                return Ok(false);
            }
        }

        // Check messages table has tier column
        let has_tier: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT FROM information_schema.columns 
                WHERE table_name = 'messages' 
                AND column_name = 'tier'
            )",
        )
        .fetch_one(&self.pool)
        .await?;

        if !has_tier {
            warn!("Messages table missing 'tier' column");
            return Ok(false);
        }

        // Check views exist
        let views = vec![
            "v_storage_tier_distribution",
            "v_deduplication_savings",
            "v_compression_efficiency",
            "v_active_storage_bonds",
            "v_active_micropayment_streams",
        ];

        for view in &views {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                    SELECT FROM information_schema.views 
                    WHERE table_schema = 'public' 
                    AND table_name = $1
                )",
            )
            .bind(view)
            .fetch_one(&self.pool)
            .await?;

            if !exists {
                warn!(view = view, "Required view does not exist");
                return Ok(false);
            }
        }

        info!("Schema verification passed");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires database connection
    async fn test_migration_runner() {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://localhost/dchat_test".to_string());

        let pool = PgPool::connect(&database_url).await.unwrap();
        let runner = MigrationRunner::new(pool);

        // Run all migrations
        let applied = runner.run_all().await.unwrap();
        assert!(applied >= 0);

        // Verify schema
        let valid = runner.verify().await.unwrap();
        assert!(valid);

        // List applied migrations
        let applied_list = runner.list_applied().await.unwrap();
        assert_eq!(applied_list.len(), MIGRATIONS.len());

        // List pending migrations (should be none)
        let pending = runner.list_pending().await.unwrap();
        assert_eq!(pending.len(), 0);
    }
}
