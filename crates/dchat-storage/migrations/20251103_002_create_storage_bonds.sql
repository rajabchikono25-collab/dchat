-- Migration: Create storage_bonds table
-- Description: Storage bonds for pre-paid storage with APY yield
-- Date: 2025-11-03

-- Storage Bonds Table
-- Users stake DCHAT tokens to reserve storage capacity
CREATE TABLE IF NOT EXISTS storage_bonds (
    -- Primary key
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    
    -- Owner
    user_id TEXT NOT NULL,  -- User's public key or identifier
    
    -- Bond economics
    amount_tokens NUMERIC(20, 8) NOT NULL CHECK (amount_tokens > 0),  -- DCHAT tokens staked
    storage_bytes BIGINT NOT NULL CHECK (storage_bytes > 0),          -- Storage reserved (bytes)
    duration_days INTEGER NOT NULL CHECK (duration_days > 0),         -- Bond duration (days)
    
    -- APY calculation fields
    apy_rate NUMERIC(5, 4) NOT NULL DEFAULT 0.05,  -- 5% APY default
    accrued_interest NUMERIC(20, 8) NOT NULL DEFAULT 0,
    
    -- Temporal fields
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    
    -- Withdrawal tracking
    withdrawn BOOLEAN NOT NULL DEFAULT FALSE,
    withdrawn_at TIMESTAMPTZ,
    withdrawn_amount NUMERIC(20, 8),  -- Principal + interest
    
    -- Constraints
    CONSTRAINT valid_expiration CHECK (expires_at > created_at),
    CONSTRAINT withdrawn_consistency CHECK (
        (withdrawn = FALSE AND withdrawn_at IS NULL AND withdrawn_amount IS NULL) OR
        (withdrawn = TRUE AND withdrawn_at IS NOT NULL AND withdrawn_amount IS NOT NULL)
    )
);

-- Index for user lookups
CREATE INDEX idx_bonds_user ON storage_bonds(user_id) WHERE withdrawn = FALSE;

-- Index for expiration processing (find expiring bonds)
CREATE INDEX idx_bonds_expires ON storage_bonds(expires_at) WHERE withdrawn = FALSE;

-- Index for withdrawal queries
CREATE INDEX idx_bonds_withdrawn ON storage_bonds(withdrawn, withdrawn_at);

-- Index for analytics (active bonds by user)
CREATE INDEX idx_bonds_active_user ON storage_bonds(user_id, created_at DESC) WHERE withdrawn = FALSE;

-- Index for total staked amount queries
CREATE INDEX idx_bonds_amount ON storage_bonds(amount_tokens) WHERE withdrawn = FALSE;

-- Comments for documentation
COMMENT ON TABLE storage_bonds IS 'Storage bonds for pre-paid storage with 5% APY yield';
COMMENT ON COLUMN storage_bonds.amount_tokens IS 'DCHAT tokens staked (with 8 decimal precision)';
COMMENT ON COLUMN storage_bonds.storage_bytes IS 'Storage capacity reserved in bytes';
COMMENT ON COLUMN storage_bonds.apy_rate IS 'Annual Percentage Yield (default 5% = 0.05)';
COMMENT ON COLUMN storage_bonds.accrued_interest IS 'Interest earned so far';
COMMENT ON COLUMN storage_bonds.expires_at IS 'Bond expiration timestamp (UTC)';
