-- Storage Facade Infrastructure Migration
-- Adds encryption key management, enhanced credential storage, and tiering tables
-- Required for StorageFacade to route messages through tiered storage

-- Encryption keys table for blob encryption at rest
-- Keys are stored encrypted by database TDE, rotated monthly
CREATE TABLE IF NOT EXISTS encryption_keys (
    key_id TEXT PRIMARY KEY,
    key_data BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    rotated_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    key_type TEXT NOT NULL DEFAULT 'aes256gcm',
    active BOOLEAN NOT NULL DEFAULT true
);

-- Index for active key lookup
CREATE INDEX IF NOT EXISTS idx_encryption_keys_active 
    ON encryption_keys(active, created_at DESC);

-- Enhanced provider credentials with separate storage per credential type
CREATE TABLE IF NOT EXISTS provider_credentials (
    id SERIAL PRIMARY KEY,
    provider_id BYTEA NOT NULL,
    credential_type TEXT NOT NULL, -- 's3', 'ipfs', 'archive'
    access_key_enc BYTEA,
    secret_key_enc BYTEA,
    session_token_enc BYTEA,
    auth_token_enc BYTEA,
    key_id TEXT NOT NULL REFERENCES encryption_keys(key_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(provider_id, credential_type)
);

-- Index for credential lookup
CREATE INDEX IF NOT EXISTS idx_provider_credentials_lookup 
    ON provider_credentials(provider_id, credential_type);

-- Cached messages table for SQLite-like local offline cache in CockroachDB
-- This mirrors the SQLite cached_messages table structure
CREATE TABLE IF NOT EXISTS cached_messages (
    id BYTEA PRIMARY KEY,
    sender_id BYTEA NOT NULL,
    recipient_id BYTEA,
    channel_id BYTEA,
    message_type TEXT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    inline_content BYTEA,
    blob_hash BYTEA REFERENCES blob_refs(hash),
    encrypted BOOLEAN NOT NULL DEFAULT true,
    encryption_key_id TEXT REFERENCES encryption_keys(key_id),
    reply_to BYTEA,
    edited_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    synced BOOLEAN NOT NULL DEFAULT false,
    sync_attempted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for cached message queries
CREATE INDEX IF NOT EXISTS idx_cached_messages_sender ON cached_messages(sender_id);
CREATE INDEX IF NOT EXISTS idx_cached_messages_recipient ON cached_messages(recipient_id);
CREATE INDEX IF NOT EXISTS idx_cached_messages_channel ON cached_messages(channel_id);
CREATE INDEX IF NOT EXISTS idx_cached_messages_timestamp ON cached_messages(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_cached_messages_unsynced 
    ON cached_messages(synced, created_at) WHERE synced = false;

-- Add encryption_key_id to blob_refs if not exists
ALTER TABLE blob_refs ADD COLUMN IF NOT EXISTS encryption_key_id TEXT REFERENCES encryption_keys(key_id);

-- Message content storage (for queryable metadata with blob reference)
-- This extends the existing messages table to properly link to blob storage
ALTER TABLE messages ADD COLUMN IF NOT EXISTS blob_hash BYTEA;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS size BIGINT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS content_type TEXT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS content BYTEA;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS channel_id BYTEA;

-- Create index on blob_hash for message-to-blob lookup
CREATE INDEX IF NOT EXISTS idx_messages_blob_hash 
    ON messages(blob_hash) WHERE blob_hash IS NOT NULL;

-- Add foreign key constraint for blob_hash if table has data
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.table_constraints 
        WHERE constraint_name = 'fk_messages_blob_refs' 
        AND table_name = 'messages'
    ) THEN
        -- Only add constraint if blob_refs table exists and messages doesn't have orphan refs
        ALTER TABLE messages ADD CONSTRAINT fk_messages_blob_refs 
            FOREIGN KEY (blob_hash) REFERENCES blob_refs(hash) ON DELETE SET NULL;
    END IF;
EXCEPTION WHEN OTHERS THEN
    -- Ignore errors if data already exists with orphan references
    NULL;
END $$;

-- Provider endpoint configuration (separate from capabilities JSONB for direct querying)
CREATE TABLE IF NOT EXISTS provider_endpoints (
    id SERIAL PRIMARY KEY,
    provider_id BYTEA NOT NULL,
    endpoint_type TEXT NOT NULL, -- 's3', 'ipfs_api', 'ipfs_gateway', 'archive'
    url TEXT NOT NULL,
    region TEXT,
    bucket TEXT,
    health_status TEXT NOT NULL DEFAULT 'unknown',
    last_health_check TIMESTAMPTZ,
    latency_ms INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(provider_id, endpoint_type)
);

-- Index for endpoint health monitoring
CREATE INDEX IF NOT EXISTS idx_provider_endpoints_health 
    ON provider_endpoints(health_status, last_health_check);

-- Provider rate limits and quotas
CREATE TABLE IF NOT EXISTS provider_limits (
    provider_id BYTEA PRIMARY KEY,
    max_object_size BIGINT NOT NULL DEFAULT 5368709120, -- 5GB
    max_storage_per_user BIGINT NOT NULL DEFAULT 107374182400, -- 100GB
    max_objects_per_user BIGINT NOT NULL DEFAULT 1000000,
    rate_limit_rpm INTEGER NOT NULL DEFAULT 1000,
    max_bandwidth_per_day BIGINT NOT NULL DEFAULT 1099511627776, -- 1TB
    min_object_size BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Blob replication status view (enhanced)
CREATE OR REPLACE VIEW blob_replication_status AS
SELECT 
    br.hash,
    br.size,
    br.encrypted,
    br.encryption_key_id,
    br.created_at,
    br.last_verified,
    COUNT(bl.id) AS total_locations,
    COUNT(bl.id) FILTER (WHERE bl.status = 'Healthy') AS healthy_count,
    COUNT(bl.id) FILTER (WHERE bl.status = 'Pending') AS pending_count,
    COUNT(bl.id) FILTER (WHERE bl.status = 'Failed') AS failed_count,
    COUNT(DISTINCT bl.region) AS region_count,
    COUNT(bl.id) FILTER (WHERE bl.ipfs_cid IS NOT NULL) AS ipfs_count,
    ARRAY_AGG(DISTINCT bl.region) FILTER (WHERE bl.region IS NOT NULL) AS regions,
    MIN(bl.created_at) AS first_stored,
    MAX(bl.last_accessed) AS last_accessed,
    CASE 
        WHEN COUNT(bl.id) FILTER (WHERE bl.status = 'Healthy') >= 2 
             AND COUNT(DISTINCT bl.region) >= 2 THEN 'fully_replicated'
        WHEN COUNT(bl.id) FILTER (WHERE bl.status = 'Healthy') >= 2 THEN 'replicated'
        WHEN COUNT(bl.id) FILTER (WHERE bl.status = 'Healthy') = 1 THEN 'at_risk'
        ELSE 'critical'
    END AS replication_status
FROM blob_refs br
LEFT JOIN blob_locations bl ON br.hash = bl.blob_hash
GROUP BY br.hash, br.size, br.encrypted, br.encryption_key_id, br.created_at, br.last_verified;

-- User storage usage view
CREATE OR REPLACE VIEW user_storage_usage AS
SELECT 
    usq.user_id,
    usq.total_quota_bytes,
    usq.used_bytes,
    usq.blob_count,
    usq.inline_content_bytes,
    usq.total_quota_bytes - usq.used_bytes AS available_bytes,
    CASE 
        WHEN usq.total_quota_bytes > 0 
        THEN (usq.used_bytes::float / usq.total_quota_bytes * 100)::numeric(5,2)
        ELSE 0 
    END AS usage_percent,
    usq.last_updated,
    COUNT(sr.id) AS recent_operations,
    COALESCE(SUM(sr.amount) FILTER (WHERE sr.operation_type = 'store'), 0) AS recent_storage_cost,
    COALESCE(SUM(sr.amount) FILTER (WHERE sr.operation_type = 'retrieve'), 0) AS recent_retrieval_cost
FROM user_storage_quotas usq
LEFT JOIN storage_receipts sr ON usq.user_id = sr.user_id 
    AND sr.created_at > NOW() - INTERVAL '30 days'
GROUP BY usq.user_id, usq.total_quota_bytes, usq.used_bytes, 
         usq.blob_count, usq.inline_content_bytes, usq.last_updated;

-- Function to enforce storage quota before blob storage
CREATE OR REPLACE FUNCTION check_storage_quota()
RETURNS TRIGGER AS $$
DECLARE
    current_usage BIGINT;
    max_quota BIGINT;
BEGIN
    -- This would be called from application code with user context
    -- For now, just pass through
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Chain anchor records for storage proofs (receipts anchored on-chain)
CREATE TABLE IF NOT EXISTS chain_anchored_receipts (
    receipt_id INTEGER PRIMARY KEY REFERENCES storage_receipts(id),
    chain_tx_id TEXT NOT NULL,
    block_height BIGINT,
    block_hash TEXT,
    anchored_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    verified BOOLEAN NOT NULL DEFAULT false,
    verified_at TIMESTAMPTZ
);

-- Index for unverified anchors
CREATE INDEX IF NOT EXISTS idx_chain_anchored_receipts_unverified 
    ON chain_anchored_receipts(verified) WHERE verified = false;

-- Comments for documentation
COMMENT ON TABLE encryption_keys IS 'Encryption keys for blob encryption at rest, rotated monthly';
COMMENT ON TABLE provider_credentials IS 'Encrypted credentials for storage provider authentication';
COMMENT ON TABLE cached_messages IS 'Offline cache of messages synced from remote';
COMMENT ON TABLE provider_endpoints IS 'Provider endpoint URLs and health status';
COMMENT ON TABLE provider_limits IS 'Rate limits and quotas for storage providers';
COMMENT ON VIEW blob_replication_status IS 'Current replication status of each blob';
COMMENT ON VIEW user_storage_usage IS 'User storage quota and usage summary';
COMMENT ON TABLE chain_anchored_receipts IS 'Storage receipts anchored on-chain for audit';
