-- Migration: Add tier management columns to messages table
-- Description: Add storage tier tracking and content references to existing messages
-- Date: 2025-11-03

-- Add tier tracking column
-- Tracks which storage tier this message currently resides in
ALTER TABLE messages 
ADD COLUMN IF NOT EXISTS tier TEXT NOT NULL DEFAULT 'hot'
CHECK (tier IN ('hot', 'warm', 'cold', 'archive'));

-- Add S3/object storage key for cold/archive tiers
-- NULL for hot/warm tiers (stored in Redis/TiKV)
ALTER TABLE messages
ADD COLUMN IF NOT EXISTS s3_key TEXT;

-- Add reference to content_store for deduplication
-- NULL for non-deduplicated messages
ALTER TABLE messages
ADD COLUMN IF NOT EXISTS content_hash BYTEA;

-- Add tier migration timestamp
-- Tracks when message was last moved between tiers
ALTER TABLE messages
ADD COLUMN IF NOT EXISTS last_tier_migration TIMESTAMPTZ;

-- Add compression metadata (denormalized for query performance)
ALTER TABLE messages
ADD COLUMN IF NOT EXISTS compression_algorithm TEXT
CHECK (compression_algorithm IS NULL OR compression_algorithm IN ('zstd', 'brotli', 'lz4', 'none'));

ALTER TABLE messages
ADD COLUMN IF NOT EXISTS compressed_size BIGINT
CHECK (compressed_size IS NULL OR compressed_size > 0);

-- Indexes for tier management queries

-- Find messages eligible for hot→warm migration (older than 7 days)
CREATE INDEX idx_messages_tier_hot ON messages(created_at)
WHERE tier = 'hot';

-- Find messages eligible for warm→cold migration (older than 90 days)
CREATE INDEX idx_messages_tier_warm ON messages(created_at)
WHERE tier = 'warm';

-- Find messages eligible for cold→archive migration (older than 365 days)
CREATE INDEX idx_messages_tier_cold ON messages(created_at)
WHERE tier = 'cold';

-- General tier lookup index
CREATE INDEX idx_messages_tier ON messages(tier, created_at DESC);

-- Lookup messages by content hash (deduplication queries)
CREATE INDEX idx_messages_content_hash ON messages(content_hash)
WHERE content_hash IS NOT NULL;

-- S3 key lookup for cold/archive tier retrieval
CREATE INDEX idx_messages_s3_key ON messages(s3_key)
WHERE s3_key IS NOT NULL;

-- Migration tracking (for monitoring and analytics)
CREATE INDEX idx_messages_last_migration ON messages(last_tier_migration)
WHERE last_tier_migration IS NOT NULL;

-- Foreign key constraint to content_store (optional, for referential integrity)
-- Note: This may impact performance; consider using application-level checks instead
-- ALTER TABLE messages
-- ADD CONSTRAINT fk_messages_content_hash 
-- FOREIGN KEY (content_hash) REFERENCES content_store(hash)
-- ON DELETE SET NULL;

-- Comments for documentation
COMMENT ON COLUMN messages.tier IS 'Storage tier: hot (<7d), warm (7-90d), cold (90-365d), archive (>365d)';
COMMENT ON COLUMN messages.s3_key IS 'Object storage key for cold/archive tiers (NULL for hot/warm)';
COMMENT ON COLUMN messages.content_hash IS 'Reference to content_store.hash for deduplicated content';
COMMENT ON COLUMN messages.last_tier_migration IS 'Timestamp of last tier migration';
COMMENT ON COLUMN messages.compression_algorithm IS 'Compression used (denormalized from content_store)';
COMMENT ON COLUMN messages.compressed_size IS 'Compressed size in bytes (denormalized from content_store)';
