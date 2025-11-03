-- Migration: Create storage analytics views
-- Description: Materialized views for storage optimization analytics
-- Date: 2025-11-03

-- View: Storage tier distribution
-- Shows message count and size by tier
CREATE OR REPLACE VIEW v_storage_tier_distribution AS
SELECT 
    tier,
    COUNT(*) as message_count,
    SUM(LENGTH(content)) as total_original_bytes,
    SUM(COALESCE(compressed_size, LENGTH(content))) as total_stored_bytes,
    AVG(LENGTH(content)) as avg_message_size,
    MIN(created_at) as oldest_message,
    MAX(created_at) as newest_message
FROM messages
GROUP BY tier;

-- View: Deduplication savings
-- Shows savings from content deduplication
CREATE OR REPLACE VIEW v_deduplication_savings AS
SELECT 
    COUNT(DISTINCT content_hash) as unique_content_items,
    COUNT(*) as total_references,
    SUM(original_size) as total_original_bytes,
    SUM(compressed_size) as total_stored_bytes,
    SUM(original_size - compressed_size) as bytes_saved,
    ROUND(100.0 * (1 - SUM(compressed_size)::FLOAT / SUM(original_size)::FLOAT), 2) as savings_percentage
FROM content_store
WHERE ref_count > 0;

-- View: Compression efficiency by algorithm
-- Shows compression ratios for each algorithm
CREATE OR REPLACE VIEW v_compression_efficiency AS
SELECT 
    compression_algorithm,
    compression_level,
    COUNT(*) as content_items,
    SUM(original_size) as total_original_bytes,
    SUM(compressed_size) as total_compressed_bytes,
    ROUND(AVG(100.0 * compressed_size::FLOAT / original_size::FLOAT), 2) as avg_compression_ratio,
    ROUND(100.0 * (1 - SUM(compressed_size)::FLOAT / SUM(original_size)::FLOAT), 2) as total_savings_percentage
FROM content_store
GROUP BY compression_algorithm, compression_level
ORDER BY compression_algorithm, compression_level;

-- View: Active storage bonds summary
-- Shows total bonded storage capacity
CREATE OR REPLACE VIEW v_active_storage_bonds AS
SELECT 
    COUNT(*) as active_bonds,
    COUNT(DISTINCT user_id) as unique_users,
    SUM(amount_tokens) as total_staked_tokens,
    SUM(storage_bytes) as total_storage_reserved_bytes,
    SUM(accrued_interest) as total_accrued_interest,
    AVG(duration_days) as avg_duration_days,
    MIN(expires_at) as next_expiration,
    MAX(expires_at) as last_expiration
FROM storage_bonds
WHERE withdrawn = FALSE AND expires_at > NOW();

-- View: Active micropayment streams summary
-- Shows total streaming payment activity
CREATE OR REPLACE VIEW v_active_micropayment_streams AS
SELECT 
    COUNT(*) as active_streams,
    COUNT(DISTINCT sender_id) as unique_senders,
    COUNT(DISTINCT receiver_id) as unique_receivers,
    SUM(flow_rate_tokens_per_sec) as total_flow_rate,
    SUM(total_streamed) as cumulative_payments,
    SUM(balance_remaining) as total_balance_remaining,
    SUM(storage_bytes_used) as total_storage_bytes,
    SUM(bandwidth_bytes_used) as total_bandwidth_bytes,
    AVG(flow_rate_tokens_per_sec) as avg_flow_rate
FROM micropayment_streams
WHERE active = TRUE;

-- View: Tier migration candidates
-- Shows messages eligible for tier migration
CREATE OR REPLACE VIEW v_tier_migration_candidates AS
SELECT 
    'hot_to_warm' as migration_type,
    COUNT(*) as candidate_count,
    SUM(LENGTH(content)) as total_bytes
FROM messages
WHERE tier = 'hot' AND created_at < NOW() - INTERVAL '7 days'

UNION ALL

SELECT 
    'warm_to_cold' as migration_type,
    COUNT(*) as candidate_count,
    SUM(LENGTH(content)) as total_bytes
FROM messages
WHERE tier = 'warm' AND created_at < NOW() - INTERVAL '90 days'

UNION ALL

SELECT 
    'cold_to_archive' as migration_type,
    COUNT(*) as candidate_count,
    SUM(LENGTH(content)) as total_bytes
FROM messages
WHERE tier = 'cold' AND created_at < NOW() - INTERVAL '365 days';

-- View: Garbage collection candidates
-- Shows content with zero references (ready for deletion)
CREATE OR REPLACE VIEW v_garbage_collection_candidates AS
SELECT 
    hash,
    original_size,
    compressed_size,
    compression_algorithm,
    content_type,
    created_at,
    last_accessed,
    NOW() - last_accessed as idle_duration
FROM content_store
WHERE ref_count = 0
ORDER BY last_accessed ASC;

-- View: Storage cost estimation
-- Estimates monthly storage costs by tier
CREATE OR REPLACE VIEW v_storage_cost_estimation AS
SELECT 
    tier,
    COUNT(*) as message_count,
    SUM(COALESCE(compressed_size, LENGTH(content))) as total_bytes,
    ROUND(SUM(COALESCE(compressed_size, LENGTH(content)))::FLOAT / 1024 / 1024 / 1024, 2) as total_gb,
    CASE tier
        WHEN 'hot' THEN ROUND(SUM(COALESCE(compressed_size, LENGTH(content)))::FLOAT / 1024 / 1024 / 1024 * 0.23, 2)
        WHEN 'warm' THEN ROUND(SUM(COALESCE(compressed_size, LENGTH(content)))::FLOAT / 1024 / 1024 / 1024 * 0.10, 2)
        WHEN 'cold' THEN ROUND(SUM(COALESCE(compressed_size, LENGTH(content)))::FLOAT / 1024 / 1024 / 1024 * 0.023, 2)
        WHEN 'archive' THEN ROUND(SUM(COALESCE(compressed_size, LENGTH(content)))::FLOAT / 1024 / 1024 / 1024 * 0.004, 2)
    END as estimated_monthly_cost_usd
FROM messages
GROUP BY tier;

-- Comments for documentation
COMMENT ON VIEW v_storage_tier_distribution IS 'Message distribution across storage tiers';
COMMENT ON VIEW v_deduplication_savings IS 'Savings from content deduplication';
COMMENT ON VIEW v_compression_efficiency IS 'Compression ratios by algorithm';
COMMENT ON VIEW v_active_storage_bonds IS 'Active storage bond statistics';
COMMENT ON VIEW v_active_micropayment_streams IS 'Active micropayment stream statistics';
COMMENT ON VIEW v_tier_migration_candidates IS 'Messages eligible for tier migration';
COMMENT ON VIEW v_garbage_collection_candidates IS 'Content with zero references ready for deletion';
COMMENT ON VIEW v_storage_cost_estimation IS 'Estimated monthly storage costs by tier';
