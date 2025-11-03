-- Migration: Create content_store table for deduplication
-- Description: Content-addressable storage with Blake3 hashing, reference counting, and metadata
-- Date: 2025-11-03

-- Content Store Table
-- Stores deduplicated, compressed content referenced by Blake3 hash
CREATE TABLE IF NOT EXISTS content_store (
    -- Primary key: Blake3 content hash (32 bytes)
    hash BYTEA PRIMARY KEY,
    
    -- Compressed content blob
    content BYTEA NOT NULL,
    
    -- Size information
    original_size BIGINT NOT NULL,      -- Size before compression (bytes)
    compressed_size BIGINT NOT NULL,    -- Size after compression (bytes)
    
    -- Compression metadata
    compression_algorithm TEXT NOT NULL CHECK (compression_algorithm IN ('zstd', 'brotli', 'lz4', 'none')),
    compression_level INTEGER,          -- NULL for 'none' and 'lz4'
    
    -- Reference counting for garbage collection
    ref_count INTEGER NOT NULL DEFAULT 1 CHECK (ref_count >= 0),
    
    -- Content metadata
    content_type TEXT,                  -- MIME type (e.g., 'text/plain', 'image/png')
    
    -- Temporal metadata
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_accessed TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Constraints
    CONSTRAINT positive_sizes CHECK (original_size > 0 AND compressed_size > 0),
    CONSTRAINT valid_compression_ratio CHECK (compressed_size <= original_size)
);

-- Index for tier management queries (find old content to migrate)
CREATE INDEX idx_content_last_accessed ON content_store(last_accessed DESC);

-- Index for garbage collection queries (find zero-ref content)
CREATE INDEX idx_content_ref_count ON content_store(ref_count) WHERE ref_count = 0;

-- Index for storage analytics (compression efficiency)
CREATE INDEX idx_content_compression ON content_store(compression_algorithm, compression_level);

-- Index for content type filtering
CREATE INDEX idx_content_type ON content_store(content_type) WHERE content_type IS NOT NULL;

-- Comments for documentation
COMMENT ON TABLE content_store IS 'Content-addressable storage for deduplicated messages and media';
COMMENT ON COLUMN content_store.hash IS 'Blake3 cryptographic hash (32 bytes)';
COMMENT ON COLUMN content_store.ref_count IS 'Number of references; automatically garbage collected when 0';
COMMENT ON COLUMN content_store.last_accessed IS 'Used for tier migration decisions';
