-- Migration: Create micropayment_streams table
-- Description: Streaming micropayments for pay-as-you-go storage
-- Date: 2025-11-03

-- Micropayment Streams Table
-- Real-time streaming payments for storage services
CREATE TABLE IF NOT EXISTS micropayment_streams (
    -- Primary key
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    
    -- Parties
    sender_id TEXT NOT NULL,    -- Payer's public key
    receiver_id TEXT NOT NULL,  -- Storage provider's public key
    
    -- Stream economics
    flow_rate_tokens_per_sec NUMERIC(20, 12) NOT NULL CHECK (flow_rate_tokens_per_sec > 0),  -- DCHAT/second
    total_streamed NUMERIC(20, 8) NOT NULL DEFAULT 0,  -- Total paid so far
    balance_remaining NUMERIC(20, 8) NOT NULL DEFAULT 0,  -- Prepaid balance
    
    -- Temporal fields
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_payment_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    stopped_at TIMESTAMPTZ,
    
    -- Status
    active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Storage metadata (optional tracking)
    storage_bytes_used BIGINT DEFAULT 0,  -- Current storage usage
    bandwidth_bytes_used BIGINT DEFAULT 0,  -- Total bandwidth consumed
    
    -- Constraints
    CONSTRAINT valid_flow_rate CHECK (flow_rate_tokens_per_sec > 0),
    CONSTRAINT valid_balance CHECK (balance_remaining >= 0),
    CONSTRAINT valid_payments CHECK (last_payment_at >= started_at),
    CONSTRAINT stopped_consistency CHECK (
        (active = TRUE AND stopped_at IS NULL) OR
        (active = FALSE AND stopped_at IS NOT NULL)
    )
);

-- Index for sender lookups (find all outgoing streams)
CREATE INDEX idx_streams_sender ON micropayment_streams(sender_id) WHERE active = TRUE;

-- Index for receiver lookups (find all incoming streams)
CREATE INDEX idx_streams_receiver ON micropayment_streams(receiver_id) WHERE active = TRUE;

-- Index for active stream processing
CREATE INDEX idx_streams_active ON micropayment_streams(active, last_payment_at);

-- Index for payment processing (find streams needing payment)
CREATE INDEX idx_streams_payment_due ON micropayment_streams(last_payment_at, active) 
    WHERE active = TRUE AND balance_remaining > 0;

-- Index for analytics (total flow rates by receiver)
CREATE INDEX idx_streams_receiver_analytics ON micropayment_streams(receiver_id, flow_rate_tokens_per_sec)
    WHERE active = TRUE;

-- Composite index for bilateral stream lookup
CREATE INDEX idx_streams_bilateral ON micropayment_streams(sender_id, receiver_id, active);

-- Comments for documentation
COMMENT ON TABLE micropayment_streams IS 'Streaming micropayments for pay-as-you-go storage services';
COMMENT ON COLUMN micropayment_streams.flow_rate_tokens_per_sec IS 'Payment rate in DCHAT tokens per second (12 decimal precision)';
COMMENT ON COLUMN micropayment_streams.total_streamed IS 'Cumulative amount paid since stream started';
COMMENT ON COLUMN micropayment_streams.balance_remaining IS 'Prepaid balance available for streaming';
COMMENT ON COLUMN micropayment_streams.last_payment_at IS 'Last time payment was processed (for rate calculation)';
COMMENT ON COLUMN micropayment_streams.storage_bytes_used IS 'Current storage consumption by this stream';
COMMENT ON COLUMN micropayment_streams.bandwidth_bytes_used IS 'Total bandwidth consumed by this stream';
