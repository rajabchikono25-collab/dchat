# Database Migrations Quick Reference

## Quick Start

### 1. Run All Migrations

```bash
# PowerShell
$env:DATABASE_URL = "postgresql://user:pass@localhost:5432/dchat"
.\scripts\run-migrations.ps1

# Bash
export DATABASE_URL="postgresql://user:pass@localhost:5432/dchat"
./scripts/run-migrations.sh
```

### 2. Verify Schema

```bash
# PowerShell
.\scripts\run-migrations.ps1 -Verify

# Bash
./scripts/run-migrations.sh --verify
```

### 3. List Migration Status

```bash
# PowerShell
.\scripts\run-migrations.ps1 -List

# Bash
./scripts/run-migrations.sh --list
```

## Using from Rust Code

```rust
use dchat_storage::MigrationRunner;
use sqlx::PgPool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = PgPool::connect(&std::env::var("DATABASE_URL")?).await?;
    let runner = MigrationRunner::new(pool);
    
    // Run all pending migrations
    runner.run_all().await?;
    
    // Verify schema
    assert!(runner.verify().await?);
    
    Ok(())
}
```

## Migration Files

| Migration | Purpose | Impact |
|-----------|---------|--------|
| 001 | Create content_store table | Content-addressable storage |
| 002 | Create storage_bonds table | Pre-paid storage with APY |
| 003 | Create micropayment_streams table | Pay-as-you-go streaming |
| 004 | Add tier columns to messages | Tier management tracking |
| 005 | Create analytics views | 8 monitoring views |

## Key Analytics Queries

```sql
-- Total storage cost estimate
SELECT SUM(estimated_monthly_cost_usd) as cost
FROM v_storage_cost_estimation;

-- Deduplication savings
SELECT savings_percentage, bytes_saved
FROM v_deduplication_savings;

-- Messages needing migration
SELECT * FROM v_tier_migration_candidates;

-- Garbage collection queue
SELECT COUNT(*) FROM v_garbage_collection_candidates;
```

## Rollback (Emergency Only)

```sql
-- Drop everything
DROP VIEW IF EXISTS v_storage_cost_estimation CASCADE;
DROP VIEW IF EXISTS v_garbage_collection_candidates CASCADE;
DROP VIEW IF EXISTS v_tier_migration_candidates CASCADE;
DROP VIEW IF EXISTS v_active_micropayment_streams CASCADE;
DROP VIEW IF EXISTS v_active_storage_bonds CASCADE;
DROP VIEW IF EXISTS v_compression_efficiency CASCADE;
DROP VIEW IF EXISTS v_deduplication_savings CASCADE;
DROP VIEW IF EXISTS v_storage_tier_distribution CASCADE;

ALTER TABLE messages DROP COLUMN IF EXISTS tier;
ALTER TABLE messages DROP COLUMN IF EXISTS s3_key;
ALTER TABLE messages DROP COLUMN IF EXISTS content_hash;

DROP TABLE IF EXISTS micropayment_streams;
DROP TABLE IF EXISTS storage_bonds;
DROP TABLE IF EXISTS content_store;
DROP TABLE IF EXISTS _schema_migrations;
```

## Troubleshooting

### "messages table does not exist"
Skip migration 004 initially, apply it after messages table is created.

### "permission denied"
Ensure database user has CREATE TABLE privileges:
```sql
GRANT CREATE ON SCHEMA public TO your_user;
```

### "already exists" errors
Migrations are idempotent. Check `_schema_migrations` table:
```sql
SELECT * FROM _schema_migrations ORDER BY applied_at;
```

## Multi-Region Setup (CockroachDB)

```sql
ALTER DATABASE dchat SET PRIMARY REGION "us-east-1";
ALTER DATABASE dchat ADD REGION "eu-west-1";
ALTER DATABASE dchat ADD REGION "ap-southeast-1";

ALTER TABLE content_store SET LOCALITY REGIONAL BY ROW;
ALTER TABLE storage_bonds SET LOCALITY REGIONAL BY ROW;
ALTER TABLE micropayment_streams SET LOCALITY REGIONAL BY ROW;
```

## Documentation

- Full docs: `crates/dchat-storage/migrations/README.md`
- Implementation report: `DATABASE_SCHEMA_MIGRATIONS_COMPLETE.md`
- Storage optimization: `STORAGE_OPTIMIZATION_COMPLETE.md`
