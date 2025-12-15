-- Storage Provider Registry Migration
-- Creates tables for provider registration, blob references, and storage challenges
-- Part of the storage marketplace implementation

-- Storage providers table (persisted registry)
CREATE TABLE IF NOT EXISTS storage_providers (
    id BYTEA PRIMARY KEY,
    name TEXT NOT NULL,
    capabilities JSONB NOT NULL,
    stake BIGINT NOT NULL DEFAULT 0,
    total_capacity BIGINT NOT NULL DEFAULT 0,
    used_capacity BIGINT NOT NULL DEFAULT 0,
    reputation_score DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    successful_challenges BIGINT NOT NULL DEFAULT 0,
    failed_challenges BIGINT NOT NULL DEFAULT 0,
    total_earnings BIGINT NOT NULL DEFAULT 0,
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    active BOOLEAN NOT NULL DEFAULT true
);

-- Index for active providers lookup
CREATE INDEX IF NOT EXISTS idx_storage_providers_active 
    ON storage_providers(active) WHERE active = true;

-- Index for reputation-based sorting
CREATE INDEX IF NOT EXISTS idx_storage_providers_reputation 
    ON storage_providers(reputation_score DESC);

-- Messages table with blob support
CREATE TABLE IF NOT EXISTS messages (
    id BYTEA PRIMARY KEY,
    sender_id BYTEA NOT NULL,
    recipient_id BYTEA NOT NULL,
    message_type TEXT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    inline_content BYTEA,
    blob_ref JSONB,
    encrypted BOOLEAN NOT NULL DEFAULT true,
    encryption_key_id BYTEA,
    reply_to BYTEA,
    edited_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ
);

-- Indexes for message queries
CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id);
CREATE INDEX IF NOT EXISTS idx_messages_recipient ON messages(recipient_id);
CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(sender_id, recipient_id, timestamp DESC);

-- Blob references table
CREATE TABLE IF NOT EXISTS blob_refs (
    hash BYTEA PRIMARY KEY,
    size BIGINT NOT NULL,
    codec TEXT NOT NULL,
    mime_type TEXT,
    encrypted BOOLEAN NOT NULL DEFAULT false,
    locations JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_verified TIMESTAMPTZ
);

-- Index for blob verification queries
CREATE INDEX IF NOT EXISTS idx_blob_refs_verified 
    ON blob_refs(last_verified) WHERE last_verified IS NOT NULL;

-- Storage challenges table
CREATE TABLE IF NOT EXISTS storage_challenges (
    id BYTEA PRIMARY KEY,
    provider_id BYTEA NOT NULL REFERENCES storage_providers(id),
    blob_hash BYTEA NOT NULL,
    range_start BIGINT NOT NULL,
    range_end BIGINT NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deadline TIMESTAMPTZ NOT NULL,
    expected_hash BYTEA,
    status TEXT NOT NULL DEFAULT 'Pending',
    payment_amount BIGINT NOT NULL DEFAULT 0,
    slash_amount BIGINT NOT NULL DEFAULT 0
);

-- Indexes for challenge management
CREATE INDEX IF NOT EXISTS idx_storage_challenges_provider 
    ON storage_challenges(provider_id);
CREATE INDEX IF NOT EXISTS idx_storage_challenges_status 
    ON storage_challenges(status) WHERE status = 'Pending';
CREATE INDEX IF NOT EXISTS idx_storage_challenges_deadline 
    ON storage_challenges(deadline) WHERE status = 'Pending';

-- Challenge results table
CREATE TABLE IF NOT EXISTS challenge_results (
    challenge_id BYTEA PRIMARY KEY REFERENCES storage_challenges(id),
    provider_id BYTEA NOT NULL REFERENCES storage_providers(id),
    passed BOOLEAN NOT NULL,
    failure_reason TEXT,
    payment_tx_id TEXT,
    slash_tx_id TEXT,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    chain_anchor_tx TEXT
);

-- Index for provider result history
CREATE INDEX IF NOT EXISTS idx_challenge_results_provider 
    ON challenge_results(provider_id, timestamp DESC);

-- Micropayment receipts table (storage payments)
CREATE TABLE IF NOT EXISTS storage_micropayments (
    id SERIAL PRIMARY KEY,
    provider_id BYTEA NOT NULL REFERENCES storage_providers(id),
    user_id BYTEA NOT NULL,
    amount BIGINT NOT NULL,
    reason TEXT NOT NULL,
    challenge_id BYTEA REFERENCES storage_challenges(id),
    tx_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for payment history
CREATE INDEX IF NOT EXISTS idx_storage_micropayments_provider 
    ON storage_micropayments(provider_id, created_at DESC);

-- Provider slashing records
CREATE TABLE IF NOT EXISTS provider_slashing (
    id SERIAL PRIMARY KEY,
    provider_id BYTEA NOT NULL REFERENCES storage_providers(id),
    amount BIGINT NOT NULL,
    reason TEXT NOT NULL,
    challenge_id BYTEA REFERENCES storage_challenges(id),
    tx_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for slashing history
CREATE INDEX IF NOT EXISTS idx_provider_slashing_provider 
    ON provider_slashing(provider_id, created_at DESC);

-- Analytics view for provider performance
CREATE OR REPLACE VIEW provider_performance AS
SELECT 
    sp.id,
    sp.name,
    sp.reputation_score,
    sp.successful_challenges,
    sp.failed_challenges,
    CASE 
        WHEN sp.successful_challenges + sp.failed_challenges > 0 
        THEN sp.successful_challenges::float / (sp.successful_challenges + sp.failed_challenges)
        ELSE 1.0 
    END AS challenge_success_rate,
    sp.total_earnings,
    sp.used_capacity,
    sp.total_capacity,
    CASE 
        WHEN sp.total_capacity > 0 
        THEN sp.used_capacity::float / sp.total_capacity 
        ELSE 0 
    END AS capacity_utilization,
    COUNT(DISTINCT cr.challenge_id) FILTER (WHERE cr.passed) AS recent_successes,
    COUNT(DISTINCT cr.challenge_id) FILTER (WHERE NOT cr.passed) AS recent_failures
FROM storage_providers sp
LEFT JOIN challenge_results cr ON sp.id = cr.provider_id 
    AND cr.timestamp > NOW() - INTERVAL '7 days'
GROUP BY sp.id, sp.name, sp.reputation_score, sp.successful_challenges, 
    sp.failed_challenges, sp.total_earnings, sp.used_capacity, sp.total_capacity;

-- Function to update provider reputation after challenge
CREATE OR REPLACE FUNCTION update_provider_reputation()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.passed THEN
        UPDATE storage_providers
        SET 
            successful_challenges = successful_challenges + 1,
            reputation_score = LEAST(1.0, reputation_score + 0.001),
            last_seen = NOW()
        WHERE id = NEW.provider_id;
    ELSE
        UPDATE storage_providers
        SET 
            failed_challenges = failed_challenges + 1,
            reputation_score = GREATEST(0.0, reputation_score - 0.01),
            last_seen = NOW()
        WHERE id = NEW.provider_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to auto-update reputation
DROP TRIGGER IF EXISTS trigger_update_provider_reputation ON challenge_results;
CREATE TRIGGER trigger_update_provider_reputation
    AFTER INSERT ON challenge_results
    FOR EACH ROW
    EXECUTE FUNCTION update_provider_reputation();

-- Comment on tables for documentation
COMMENT ON TABLE storage_providers IS 'Registry of storage providers in the marketplace';
COMMENT ON TABLE blob_refs IS 'Content-addressable blob references with provider locations';
COMMENT ON TABLE storage_challenges IS 'Storage proof challenges sent to providers';
COMMENT ON TABLE challenge_results IS 'Results of storage challenges with payment/slash info';
COMMENT ON TABLE storage_micropayments IS 'Micropayment receipts for storage services';
COMMENT ON TABLE provider_slashing IS 'Slashing records for provider failures';
