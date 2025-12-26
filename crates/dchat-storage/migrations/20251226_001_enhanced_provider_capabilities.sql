-- Enhanced Provider Capabilities Migration
-- Extends storage_providers with detailed endpoint/auth/pricing configuration
-- Adds blob location tracking and tiering support

-- Add detailed provider configuration columns
ALTER TABLE storage_providers ADD COLUMN IF NOT EXISTS description TEXT;
ALTER TABLE storage_providers ADD COLUMN IF NOT EXISTS operator_address TEXT NOT NULL DEFAULT '';
ALTER TABLE storage_providers ADD COLUMN IF NOT EXISTS primary_region TEXT NOT NULL DEFAULT 'us-east-1';
ALTER TABLE storage_providers ADD COLUMN IF NOT EXISTS additional_regions TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE storage_providers ADD COLUMN IF NOT EXISTS version TEXT NOT NULL DEFAULT '0.1.0';
ALTER TABLE storage_providers ADD COLUMN IF NOT EXISTS registration_tx_id TEXT;
ALTER TABLE storage_providers ADD COLUMN IF NOT EXISTS last_chain_sync TIMESTAMPTZ;

-- Rename columns for consistency
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'storage_providers' AND column_name = 'stake') THEN
        ALTER TABLE storage_providers RENAME COLUMN stake TO stake_amount;
    END IF;
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'storage_providers' AND column_name = 'active') THEN
        ALTER TABLE storage_providers RENAME COLUMN active TO is_active;
    END IF;
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'storage_providers' AND column_name = 'used_capacity') THEN
        ALTER TABLE storage_providers RENAME COLUMN used_capacity TO used_storage;
    END IF;
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'storage_providers' AND column_name = 'total_capacity') THEN
        -- Keep as total_capacity but ensure we have both names via view
        NULL;
    END IF;
END $$;

-- Create index for region-based lookups
CREATE INDEX IF NOT EXISTS idx_storage_providers_region 
    ON storage_providers(primary_region);

-- Provider credentials table (encrypted, separate from public registry)
-- Stores S3/IPFS authentication credentials with encryption
CREATE TABLE IF NOT EXISTS provider_credentials (
    provider_id BYTEA PRIMARY KEY REFERENCES storage_providers(id),
    s3_access_key_encrypted BYTEA,
    s3_secret_key_encrypted BYTEA,
    s3_session_token_encrypted BYTEA,
    ipfs_auth_token_encrypted BYTEA,
    archive_access_key_encrypted BYTEA,
    archive_secret_key_encrypted BYTEA,
    encryption_key_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Blob locations table (tracks where each blob is stored)
CREATE TABLE IF NOT EXISTS blob_locations (
    id SERIAL PRIMARY KEY,
    blob_hash BYTEA NOT NULL REFERENCES blob_refs(hash),
    provider_id BYTEA NOT NULL REFERENCES storage_providers(id),
    s3_key TEXT,
    s3_bucket TEXT,
    ipfs_cid TEXT,
    archive_key TEXT,
    storage_class TEXT NOT NULL DEFAULT 'Standard',
    region TEXT,
    status TEXT NOT NULL DEFAULT 'Pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_accessed TIMESTAMPTZ,
    last_challenged TIMESTAMPTZ,
    failed_challenges INTEGER NOT NULL DEFAULT 0,
    UNIQUE(blob_hash, provider_id)
);

-- Indexes for blob location queries
CREATE INDEX IF NOT EXISTS idx_blob_locations_blob ON blob_locations(blob_hash);
CREATE INDEX IF NOT EXISTS idx_blob_locations_provider ON blob_locations(provider_id);
CREATE INDEX IF NOT EXISTS idx_blob_locations_status ON blob_locations(status);
CREATE INDEX IF NOT EXISTS idx_blob_locations_region ON blob_locations(region);

-- Storage tier migration tracking table
-- Used for "move/replicate blob locations" rather than just updating a tier column
CREATE TABLE IF NOT EXISTS blob_tier_migrations (
    id SERIAL PRIMARY KEY,
    blob_hash BYTEA NOT NULL REFERENCES blob_refs(hash),
    source_location_id INTEGER REFERENCES blob_locations(id),
    target_provider_id BYTEA REFERENCES storage_providers(id),
    target_storage_class TEXT NOT NULL,
    target_region TEXT,
    status TEXT NOT NULL DEFAULT 'Pending',
    initiated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    error_message TEXT
);

-- Index for pending migrations
CREATE INDEX IF NOT EXISTS idx_blob_tier_migrations_status 
    ON blob_tier_migrations(status) WHERE status = 'Pending';

-- Provider pricing history table
CREATE TABLE IF NOT EXISTS provider_pricing_history (
    id SERIAL PRIMARY KEY,
    provider_id BYTEA NOT NULL REFERENCES storage_providers(id),
    s3_price_per_gb_month BIGINT NOT NULL,
    ipfs_price_per_gb_month BIGINT,
    archive_price_per_gb_month BIGINT,
    retrieval_price_per_gb BIGINT,
    effective_from TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    effective_until TIMESTAMPTZ
);

-- Index for current pricing lookup
CREATE INDEX IF NOT EXISTS idx_provider_pricing_current 
    ON provider_pricing_history(provider_id, effective_from DESC);

-- User storage quota tracking
CREATE TABLE IF NOT EXISTS user_storage_quotas (
    user_id BYTEA PRIMARY KEY,
    total_quota_bytes BIGINT NOT NULL DEFAULT 10737418240, -- 10 GB default
    used_bytes BIGINT NOT NULL DEFAULT 0,
    blob_count BIGINT NOT NULL DEFAULT 0,
    inline_content_bytes BIGINT NOT NULL DEFAULT 0,
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Function to update user quota on blob storage
CREATE OR REPLACE FUNCTION update_user_quota_on_blob()
RETURNS TRIGGER AS $$
BEGIN
    -- This would be called when a new blob_ref is created
    -- In practice, we update via application code since blob_refs don't track user
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Storage receipts table (micropayment receipts for audit)
CREATE TABLE IF NOT EXISTS storage_receipts (
    id SERIAL PRIMARY KEY,
    user_id BYTEA NOT NULL,
    provider_id BYTEA NOT NULL REFERENCES storage_providers(id),
    blob_hash BYTEA REFERENCES blob_refs(hash),
    operation_type TEXT NOT NULL, -- 'store', 'retrieve', 'replicate', 'challenge_reward'
    amount BIGINT NOT NULL,
    tx_id TEXT,
    chain_anchor_tx TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for receipt queries
CREATE INDEX IF NOT EXISTS idx_storage_receipts_user 
    ON storage_receipts(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_storage_receipts_provider 
    ON storage_receipts(provider_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_storage_receipts_blob 
    ON storage_receipts(blob_hash) WHERE blob_hash IS NOT NULL;

-- View for provider storage summary
CREATE OR REPLACE VIEW provider_storage_summary AS
SELECT 
    sp.id AS provider_id,
    sp.name,
    sp.primary_region,
    sp.reputation_score,
    sp.stake_amount,
    COUNT(DISTINCT bl.blob_hash) AS stored_blobs,
    COALESCE(SUM(br.size), 0) AS total_stored_bytes,
    COUNT(DISTINCT bl.blob_hash) FILTER (WHERE bl.status = 'Healthy') AS healthy_blobs,
    COUNT(DISTINCT bl.blob_hash) FILTER (WHERE bl.ipfs_cid IS NOT NULL) AS ipfs_pinned_blobs,
    COALESCE(sp.capabilities->'object_s3'->>'price_per_gb_month', '0')::BIGINT AS s3_price,
    COALESCE(sp.capabilities->'ipfs_pinning'->>'price_per_gb_month', '0')::BIGINT AS ipfs_price
FROM storage_providers sp
LEFT JOIN blob_locations bl ON sp.id = bl.provider_id
LEFT JOIN blob_refs br ON bl.blob_hash = br.hash
WHERE sp.is_active = true
GROUP BY sp.id, sp.name, sp.primary_region, sp.reputation_score, sp.stake_amount, sp.capabilities;

-- View for blob health status
CREATE OR REPLACE VIEW blob_health_status AS
SELECT 
    br.hash,
    br.size,
    br.encrypted,
    br.created_at,
    COUNT(bl.id) AS total_locations,
    COUNT(bl.id) FILTER (WHERE bl.status = 'Healthy') AS healthy_locations,
    COUNT(bl.id) FILTER (WHERE bl.ipfs_cid IS NOT NULL) AS ipfs_locations,
    COUNT(DISTINCT bl.region) AS region_diversity,
    MAX(bl.last_challenged) AS last_challenged,
    MIN(bl.failed_challenges) AS min_failed_challenges,
    MAX(bl.failed_challenges) AS max_failed_challenges,
    CASE 
        WHEN COUNT(bl.id) FILTER (WHERE bl.status = 'Healthy') >= 2 THEN 'healthy'
        WHEN COUNT(bl.id) FILTER (WHERE bl.status = 'Healthy') = 1 THEN 'at_risk'
        ELSE 'critical'
    END AS health_status
FROM blob_refs br
LEFT JOIN blob_locations bl ON br.hash = bl.blob_hash
GROUP BY br.hash, br.size, br.encrypted, br.created_at;

-- Comments for documentation
COMMENT ON TABLE provider_credentials IS 'Encrypted authentication credentials for storage providers';
COMMENT ON TABLE blob_locations IS 'Tracks storage locations for each blob across providers';
COMMENT ON TABLE blob_tier_migrations IS 'Tracks blob migration/replication between storage tiers and providers';
COMMENT ON TABLE storage_receipts IS 'Auditable receipts for storage operations and payments';
COMMENT ON VIEW provider_storage_summary IS 'Summary of each provider storage utilization and pricing';
COMMENT ON VIEW blob_health_status IS 'Health status of each blob based on replica availability';
