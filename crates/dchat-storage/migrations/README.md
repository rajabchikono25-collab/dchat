# Database Migrations for Storage Optimization

This directory contains SQL migration files for the dchat storage optimization system.

## Migration Order

Migrations should be applied in numerical order:

1. **20251103_001_create_content_store.sql**
   - Creates `content_store` table for content-addressable storage
   - Blake3 hashing, reference counting, compression metadata
   - Indexes for tier management and garbage collection

2. **20251103_002_create_storage_bonds.sql**
   - Creates `storage_bonds` table for pre-paid storage
   - Storage bonds with 5% APY yield
   - Indexes for user lookups, expiration processing, analytics

3. **20251103_003_create_micropayment_streams.sql**
   - Creates `micropayment_streams` table for pay-as-you-go
   - Streaming micropayments with flow rates
   - Indexes for sender/receiver lookups, payment processing

4. **20251103_004_add_messages_tier_columns.sql**
   - Adds tier management columns to existing `messages` table
   - Columns: `tier`, `s3_key`, `content_hash`, `last_tier_migration`
   - Indexes for tier migration queries

5. **20251103_005_create_analytics_views.sql**
   - Creates 8 analytics views for monitoring
   - Deduplication savings, compression efficiency, cost estimation
   - Migration candidates, garbage collection candidates

## Database Compatibility

These migrations are designed for **PostgreSQL** (CockroachDB compatible).

### PostgreSQL-specific features used:
- `BYTEA` type for binary data
- `TIMESTAMPTZ` for timezone-aware timestamps
- `UUID` with `gen_random_uuid()`
- `NUMERIC` for high-precision decimals
- `CHECK` constraints
- Partial indexes with `WHERE` clauses
- `COMMENT ON` statements

### For other databases:

#### SQLite adaptation:
```sql
-- Replace BYTEA with BLOB
-- Replace TIMESTAMPTZ with TEXT or INTEGER (Unix timestamp)
-- Replace UUID with TEXT or use custom UUID generation
-- Replace NUMERIC with REAL
-- Remove COMMENT ON statements (not supported)
-- Simplify CHECK constraints (limited support)
```

#### MySQL/MariaDB adaptation:
```sql
-- Replace BYTEA with VARBINARY or BLOB
-- Replace TIMESTAMPTZ with TIMESTAMP
-- Use CHAR(36) for UUIDs with UUID() function
-- Replace NUMERIC with DECIMAL
-- Use COMMENT = 'text' syntax instead of COMMENT ON
```

## Applying Migrations

### Using sqlx-cli (recommended):

```bash
# Install sqlx-cli
cargo install sqlx-cli --no-default-features --features postgres

# Set database URL
export DATABASE_URL="postgresql://user:pass@localhost:5432/dchat"

# Run all migrations
sqlx migrate run --source crates/dchat-storage/migrations

# Revert last migration
sqlx migrate revert --source crates/dchat-storage/migrations
```

### Using psql directly:

```bash
psql -U user -d dchat -f crates/dchat-storage/migrations/20251103_001_create_content_store.sql
psql -U user -d dchat -f crates/dchat-storage/migrations/20251103_002_create_storage_bonds.sql
psql -U user -d dchat -f crates/dchat-storage/migrations/20251103_003_create_micropayment_streams.sql
psql -U user -d dchat -f crates/dchat-storage/migrations/20251103_004_add_messages_tier_columns.sql
psql -U user -d dchat -f crates/dchat-storage/migrations/20251103_005_create_analytics_views.sql
```

### Using DBeaver or other GUI tools:

1. Connect to your CockroachDB/PostgreSQL database
2. Open each migration file in order
3. Execute the SQL statements
4. Verify indexes and constraints were created

## Verification Queries

After applying migrations, verify the schema:

```sql
-- Check tables exist
SELECT tablename FROM pg_tables 
WHERE schemaname = 'public' 
  AND tablename IN ('content_store', 'storage_bonds', 'micropayment_streams');

-- Check messages columns were added
SELECT column_name, data_type 
FROM information_schema.columns 
WHERE table_name = 'messages' 
  AND column_name IN ('tier', 's3_key', 'content_hash', 'last_tier_migration');

-- Check indexes
SELECT indexname FROM pg_indexes 
WHERE tablename IN ('content_store', 'storage_bonds', 'micropayment_streams', 'messages')
ORDER BY tablename, indexname;

-- Check views
SELECT viewname FROM pg_views 
WHERE schemaname = 'public' 
  AND viewname LIKE 'v_%';
```

## Analytics Queries

Use the analytics views to monitor storage optimization:

```sql
-- Storage tier distribution
SELECT * FROM v_storage_tier_distribution;

-- Deduplication savings
SELECT * FROM v_deduplication_savings;

-- Compression efficiency
SELECT * FROM v_compression_efficiency;

-- Active storage bonds
SELECT * FROM v_active_storage_bonds;

-- Active micropayment streams
SELECT * FROM v_active_micropayment_streams;

-- Tier migration candidates
SELECT * FROM v_tier_migration_candidates;

-- Garbage collection candidates (top 10)
SELECT * FROM v_garbage_collection_candidates LIMIT 10;

-- Storage cost estimation
SELECT 
    tier,
    total_gb,
    estimated_monthly_cost_usd,
    message_count
FROM v_storage_cost_estimation
ORDER BY estimated_monthly_cost_usd DESC;

-- Total estimated monthly cost
SELECT SUM(estimated_monthly_cost_usd) as total_monthly_cost_usd
FROM v_storage_cost_estimation;
```

## Rollback Migrations

To rollback migrations in reverse order:

```sql
-- Drop analytics views
DROP VIEW IF EXISTS v_storage_cost_estimation;
DROP VIEW IF EXISTS v_garbage_collection_candidates;
DROP VIEW IF EXISTS v_tier_migration_candidates;
DROP VIEW IF EXISTS v_active_micropayment_streams;
DROP VIEW IF EXISTS v_active_storage_bonds;
DROP VIEW IF EXISTS v_compression_efficiency;
DROP VIEW IF EXISTS v_deduplication_savings;
DROP VIEW IF EXISTS v_storage_tier_distribution;

-- Remove messages columns
ALTER TABLE messages DROP COLUMN IF EXISTS compressed_size;
ALTER TABLE messages DROP COLUMN IF EXISTS compression_algorithm;
ALTER TABLE messages DROP COLUMN IF EXISTS last_tier_migration;
ALTER TABLE messages DROP COLUMN IF EXISTS content_hash;
ALTER TABLE messages DROP COLUMN IF EXISTS s3_key;
ALTER TABLE messages DROP COLUMN IF EXISTS tier;

-- Drop tables
DROP TABLE IF EXISTS micropayment_streams;
DROP TABLE IF EXISTS storage_bonds;
DROP TABLE IF EXISTS content_store;
```

## Performance Considerations

### Index Maintenance

CockroachDB automatically maintains indexes. For PostgreSQL:

```sql
-- Reindex after large data loads
REINDEX TABLE content_store;
REINDEX TABLE storage_bonds;
REINDEX TABLE micropayment_streams;
REINDEX TABLE messages;

-- Analyze for query optimization
ANALYZE content_store;
ANALYZE storage_bonds;
ANALYZE micropayment_streams;
ANALYZE messages;
```

### Partition Tables (Optional)

For very large deployments, consider partitioning:

```sql
-- Partition content_store by created_at (monthly)
CREATE TABLE content_store_2025_11 PARTITION OF content_store
FOR VALUES FROM ('2025-11-01') TO ('2025-12-01');

-- Partition messages by tier
CREATE TABLE messages_hot PARTITION OF messages
FOR VALUES IN ('hot');

CREATE TABLE messages_warm PARTITION OF messages
FOR VALUES IN ('warm');
```

## Multi-Region Deployment

For CockroachDB multi-region setup:

```sql
-- Set database region
ALTER DATABASE dchat SET PRIMARY REGION "us-east-1";
ALTER DATABASE dchat ADD REGION "eu-west-1";
ALTER DATABASE dchat ADD REGION "ap-southeast-1";

-- Set table localities for low-latency access
ALTER TABLE content_store SET LOCALITY REGIONAL BY ROW;
ALTER TABLE storage_bonds SET LOCALITY REGIONAL BY ROW;
ALTER TABLE micropayment_streams SET LOCALITY REGIONAL BY ROW;
ALTER TABLE messages SET LOCALITY REGIONAL BY ROW;
```

## Troubleshooting

### Migration fails on messages table:

If `messages` table doesn't exist yet, comment out migration 004 and apply it later:

```bash
# Skip messages migration initially
# Apply it after messages table is created in main schema
```

### Foreign key constraint issues:

The `content_hash` foreign key to `content_store` is commented out by default for performance. Enable it only if referential integrity is critical:

```sql
ALTER TABLE messages
ADD CONSTRAINT fk_messages_content_hash 
FOREIGN KEY (content_hash) REFERENCES content_store(hash)
ON DELETE SET NULL;
```

### View creation fails:

If views fail due to missing columns, ensure migrations 001-004 completed successfully first.

## Next Steps

After applying migrations:

1. **Integration**: Update `dchat-storage` Rust code to use new schema
2. **Testing**: Run integration tests with real database
3. **Monitoring**: Set up Grafana dashboards using analytics views
4. **Backup**: Configure automated database backups
5. **Deployment**: Deploy schema to all 3 CockroachDB regions

## Resources

- [CockroachDB SQL Reference](https://www.cockroachlabs.com/docs/stable/sql-statements.html)
- [sqlx Migrations Guide](https://github.com/launchbadge/sqlx/blob/main/sqlx-cli/README.md)
- [PostgreSQL Documentation](https://www.postgresql.org/docs/)
