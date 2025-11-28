// crates/dchat-storage/src/compression.rs
//! Compression wrapper for storage optimization using Zstd and Brotli.
//!
//! Provides automatic compression/decompression with adaptive algorithm selection
//! based on content type and size.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// Compression engine with multiple algorithm support
pub struct CompressionEngine;

/// Compression algorithm selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression
    None,
    /// Zstd (fast, good ratio) - recommended for text
    Zstd,
    /// Brotli (slower, better ratio) - recommended for static content
    Brotli,
    /// LZ4 (fastest, lower ratio) - recommended for hot tier
    Lz4,
}

/// Compression level (1-9, higher = better compression but slower)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressionLevel(pub u8);

impl CompressionLevel {
    /// Fast compression (level 1-3)
    pub const FAST: Self = Self(3);
    /// Balanced compression (level 4-6)
    pub const BALANCED: Self = Self(5);
    /// Maximum compression (level 7-9)
    pub const MAX: Self = Self(9);
}

/// Compression configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Algorithm to use
    pub algorithm: CompressionAlgorithm,
    /// Compression level
    pub level: CompressionLevel,
    /// Minimum size to compress (bytes)
    pub min_size_bytes: usize,
    /// Maximum size to compress (bytes) - very large files may timeout
    pub max_size_bytes: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Zstd,
            level: CompressionLevel::FAST,
            min_size_bytes: 512,               // Don't compress tiny content
            max_size_bytes: 100 * 1024 * 1024, // 100MB max
        }
    }
}

/// Compression result with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionResult {
    /// Compressed data
    pub data: Vec<u8>,
    /// Original size
    pub original_size: usize,
    /// Compressed size
    pub compressed_size: usize,
    /// Compression ratio (compressed / original)
    pub ratio: f64,
    /// Algorithm used
    pub algorithm: CompressionAlgorithm,
    /// Level used
    pub level: CompressionLevel,
}

impl CompressionEngine {
    /// Compress data with Zstd
    pub fn compress_zstd(data: &[u8], level: u8) -> Result<Vec<u8>, CompressionError> {
        let level = level.clamp(1, 22) as i32; // Zstd supports levels 1-22

        let mut encoder = zstd::Encoder::new(Vec::new(), level)
            .map_err(|e| CompressionError::ZstdError(e.to_string()))?;

        encoder
            .write_all(data)
            .map_err(|e| CompressionError::IoError(e.to_string()))?;

        encoder
            .finish()
            .map_err(|e| CompressionError::ZstdError(e.to_string()))
    }

    /// Decompress Zstd data
    pub fn decompress_zstd(data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let mut decoder =
            zstd::Decoder::new(data).map_err(|e| CompressionError::ZstdError(e.to_string()))?;

        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| CompressionError::IoError(e.to_string()))?;

        Ok(decompressed)
    }

    /// Compress data with Brotli
    pub fn compress_brotli(data: &[u8], level: u8) -> Result<Vec<u8>, CompressionError> {
        let level = level.clamp(1, 11) as u32; // Brotli supports levels 1-11

        let mut compressed = Vec::new();
        let mut encoder = brotli::CompressorWriter::new(
            &mut compressed,
            4096, // buffer size
            level,
            22, // window size (default)
        );

        encoder
            .write_all(data)
            .map_err(|e| CompressionError::IoError(e.to_string()))?;

        drop(encoder); // Flush
        Ok(compressed)
    }

    /// Decompress Brotli data
    pub fn decompress_brotli(data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let mut decompressed = Vec::new();
        let mut decoder = brotli::Decompressor::new(data, 4096);

        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| CompressionError::IoError(e.to_string()))?;

        Ok(decompressed)
    }

    /// Compress data with LZ4
    pub fn compress_lz4(data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        Ok(lz4_flex::compress_prepend_size(data))
    }

    /// Decompress LZ4 data
    pub fn decompress_lz4(data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        lz4_flex::decompress_size_prepended(data)
            .map_err(|e| CompressionError::Lz4Error(format!("{:?}", e)))
    }

    /// Compress with automatic algorithm selection
    pub fn compress(
        data: &[u8],
        config: &CompressionConfig,
    ) -> Result<CompressionResult, CompressionError> {
        // Check size bounds
        if data.len() < config.min_size_bytes {
            return Ok(CompressionResult {
                data: data.to_vec(),
                original_size: data.len(),
                compressed_size: data.len(),
                ratio: 1.0,
                algorithm: CompressionAlgorithm::None,
                level: config.level,
            });
        }

        if data.len() > config.max_size_bytes {
            return Err(CompressionError::TooLarge(data.len()));
        }

        // Compress with selected algorithm
        let compressed = match config.algorithm {
            CompressionAlgorithm::None => data.to_vec(),
            CompressionAlgorithm::Zstd => Self::compress_zstd(data, config.level.0)?,
            CompressionAlgorithm::Brotli => Self::compress_brotli(data, config.level.0)?,
            CompressionAlgorithm::Lz4 => Self::compress_lz4(data)?,
        };

        let compressed_size = compressed.len();
        let ratio = compressed_size as f64 / data.len() as f64;

        Ok(CompressionResult {
            data: compressed,
            original_size: data.len(),
            compressed_size,
            ratio,
            algorithm: config.algorithm,
            level: config.level,
        })
    }

    /// Decompress with algorithm detection
    pub fn decompress(
        data: &[u8],
        algorithm: CompressionAlgorithm,
    ) -> Result<Vec<u8>, CompressionError> {
        match algorithm {
            CompressionAlgorithm::None => Ok(data.to_vec()),
            CompressionAlgorithm::Zstd => Self::decompress_zstd(data),
            CompressionAlgorithm::Brotli => Self::decompress_brotli(data),
            CompressionAlgorithm::Lz4 => Self::decompress_lz4(data),
        }
    }

    /// Select best algorithm for content type
    pub fn select_algorithm(content_type: &str, size: usize) -> CompressionAlgorithm {
        // Hot tier / real-time: Use LZ4 for speed
        if size < 10 * 1024 {
            // < 10KB
            return CompressionAlgorithm::Lz4;
        }

        match content_type {
            // Text-based: Zstd with good compression
            "text/plain" | "text/html" | "text/css" | "text/javascript" | "application/json"
            | "application/xml" => CompressionAlgorithm::Zstd,

            // Static content: Brotli for maximum compression
            "text/markdown" | "text/csv" => CompressionAlgorithm::Brotli,

            // Binary / media: Often already compressed, use LZ4 or none
            "image/jpeg" | "image/png" | "video/mp4" | "audio/mp3" | "application/zip"
            | "application/gzip" => CompressionAlgorithm::None,

            // Default: Zstd for balanced performance
            _ => CompressionAlgorithm::Zstd,
        }
    }

    /// Benchmark compression algorithms on sample data
    pub fn benchmark(data: &[u8]) -> BenchmarkResult {
        use std::time::Instant;

        let mut results = Vec::new();

        // Benchmark Zstd
        for level in [1, 3, 5, 9] {
            let start = Instant::now();
            if let Ok(compressed) = Self::compress_zstd(data, level) {
                let duration = start.elapsed();
                results.push(AlgorithmResult {
                    algorithm: CompressionAlgorithm::Zstd,
                    level: CompressionLevel(level),
                    compressed_size: compressed.len(),
                    compression_time_ms: duration.as_millis() as u64,
                    ratio: compressed.len() as f64 / data.len() as f64,
                });
            }
        }

        // Benchmark Brotli
        for level in [1, 5, 9] {
            let start = Instant::now();
            if let Ok(compressed) = Self::compress_brotli(data, level) {
                let duration = start.elapsed();
                results.push(AlgorithmResult {
                    algorithm: CompressionAlgorithm::Brotli,
                    level: CompressionLevel(level),
                    compressed_size: compressed.len(),
                    compression_time_ms: duration.as_millis() as u64,
                    ratio: compressed.len() as f64 / data.len() as f64,
                });
            }
        }

        // Benchmark LZ4
        let start = Instant::now();
        if let Ok(compressed) = Self::compress_lz4(data) {
            let duration = start.elapsed();
            results.push(AlgorithmResult {
                algorithm: CompressionAlgorithm::Lz4,
                level: CompressionLevel(1),
                compressed_size: compressed.len(),
                compression_time_ms: duration.as_millis() as u64,
                ratio: compressed.len() as f64 / data.len() as f64,
            });
        }

        BenchmarkResult {
            original_size: data.len(),
            results,
        }
    }
}

/// Benchmark result for compression algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub original_size: usize,
    pub results: Vec<AlgorithmResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgorithmResult {
    pub algorithm: CompressionAlgorithm,
    pub level: CompressionLevel,
    pub compressed_size: usize,
    pub compression_time_ms: u64,
    pub ratio: f64,
}

/// Compression errors
#[derive(Debug, Clone)]
pub enum CompressionError {
    ZstdError(String),
    BrotliError(String),
    Lz4Error(String),
    IoError(String),
    TooLarge(usize),
    UnsupportedAlgorithm,
}

impl std::fmt::Display for CompressionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZstdError(e) => write!(f, "Zstd error: {}", e),
            Self::BrotliError(e) => write!(f, "Brotli error: {}", e),
            Self::Lz4Error(e) => write!(f, "LZ4 error: {}", e),
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::TooLarge(size) => write!(f, "Content too large: {} bytes", size),
            Self::UnsupportedAlgorithm => write!(f, "Unsupported compression algorithm"),
        }
    }
}

impl std::error::Error for CompressionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zstd_compression() {
        let data = b"Hello, World! This is a test message that should compress well because it has repeated patterns.";

        let compressed = CompressionEngine::compress_zstd(data, 3).unwrap();
        assert!(
            compressed.len() < data.len(),
            "Compressed should be smaller"
        );

        let decompressed = CompressionEngine::decompress_zstd(&compressed).unwrap();
        assert_eq!(decompressed, data, "Round-trip should preserve data");
    }

    #[test]
    fn test_brotli_compression() {
        let data = b"Brotli compression test with some repeated patterns. Brotli is great for text compression!";

        let compressed = CompressionEngine::compress_brotli(data, 5).unwrap();
        assert!(
            compressed.len() < data.len(),
            "Compressed should be smaller"
        );

        let decompressed = CompressionEngine::decompress_brotli(&compressed).unwrap();
        assert_eq!(decompressed, data, "Round-trip should preserve data");
    }

    #[test]
    fn test_lz4_compression() {
        let data = b"LZ4 is fast! LZ4 is fast! LZ4 is fast!";

        let compressed = CompressionEngine::compress_lz4(data).unwrap();
        let decompressed = CompressionEngine::decompress_lz4(&compressed).unwrap();

        assert_eq!(decompressed, data, "Round-trip should preserve data");
    }

    #[test]
    fn test_algorithm_selection() {
        // Large text file should use Zstd
        assert_eq!(
            CompressionEngine::select_algorithm("text/plain", 100 * 1024),
            CompressionAlgorithm::Zstd
        );

        // Large JPEG (already compressed) should return None
        assert_eq!(
            CompressionEngine::select_algorithm("image/jpeg", 100 * 1024),
            CompressionAlgorithm::None
        );

        // Small text file (< 10KB) should use LZ4 for speed
        assert_eq!(
            CompressionEngine::select_algorithm("text/plain", 5 * 1024),
            CompressionAlgorithm::Lz4
        );
    }

    #[test]
    fn test_compression_config() {
        // Use data with repeated patterns that will actually compress well
        let data = b"This is a test message that repeats. This is a test message that repeats. \
                     This is a test message that repeats. This is a test message that repeats. \
                     This is a test message that repeats. This is a test message that repeats.";
        let config = CompressionConfig {
            algorithm: CompressionAlgorithm::Zstd,
            level: CompressionLevel::FAST,
            min_size_bytes: 10,
            max_size_bytes: 1024 * 1024,
        };

        let result = CompressionEngine::compress(data, &config).unwrap();
        assert!(result.ratio < 1.0, "Should achieve some compression");
        assert_eq!(result.algorithm, CompressionAlgorithm::Zstd);
    }
}
