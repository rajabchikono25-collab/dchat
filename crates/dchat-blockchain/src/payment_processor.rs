//! Payment Processor Background Service
//!
//! This module implements a production-grade background job for:
//! - Processing pending micropayments from storage streams
//! - Settling payment channel balances on the currency chain
//! - Handling payment failures with retry logic
//! - Suspending streams when funds are insufficient

use crate::currency_chain::CurrencyChainClient;
use crate::payment_channels::PaymentChannelManager;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{watch, RwLock};
use tokio::time::{interval, Duration};
use uuid::Uuid;

/// Payment processor errors
#[derive(Error, Debug)]
pub enum PaymentProcessorError {
    #[error("Insufficient funds for payment")]
    InsufficientFunds,

    #[error("Payment stream not found: {0}")]
    StreamNotFound(String),

    #[error("Payment stream suspended: {0}")]
    StreamSuspended(String),

    #[error("Currency chain error: {0}")]
    CurrencyChainError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Payment receipt for completed payments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentReceipt {
    /// Unique receipt ID
    pub receipt_id: Uuid,
    /// Stream this payment was for
    pub stream_id: String,
    /// Payer ID
    pub payer: UserId,
    /// Payee ID  
    pub payee: UserId,
    /// Amount paid
    pub amount: u64,
    /// Currency chain transaction ID
    pub tx_id: String,
    /// Payment timestamp
    pub timestamp: DateTime<Utc>,
}

/// Status of a payment stream
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamStatus {
    /// Stream is active and processing payments
    Active,
    /// Stream is temporarily suspended (insufficient funds)
    Suspended,
    /// Stream has completed (max payments reached or manually closed)
    Completed,
    /// Stream was cancelled
    Cancelled,
}

/// A micropayment stream for ongoing services (e.g., storage)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentStream {
    /// Unique stream identifier
    pub stream_id: String,
    /// Payer (service consumer)
    pub payer: UserId,
    /// Payee (service provider)
    pub payee: UserId,
    /// Payment amount per interval
    pub amount_per_interval: u64,
    /// Payment interval in seconds
    pub interval_seconds: u64,
    /// Total amount paid so far
    pub total_paid: u64,
    /// Maximum total payment (0 = unlimited)
    pub max_total: u64,
    /// Current stream status
    pub status: StreamStatus,
    /// When stream was created
    pub created_at: DateTime<Utc>,
    /// Last payment timestamp
    pub last_payment: Option<DateTime<Utc>>,
    /// Next scheduled payment
    pub next_payment: DateTime<Utc>,
    /// Number of consecutive failures
    pub failure_count: u32,
    /// Associated payment channel (if any)
    pub channel_id: Option<String>,
}

impl PaymentStream {
    /// Create a new payment stream
    pub fn new(
        payer: UserId,
        payee: UserId,
        amount_per_interval: u64,
        interval_seconds: u64,
        max_total: u64,
    ) -> Self {
        let now = Utc::now();
        Self {
            stream_id: Uuid::new_v4().to_string(),
            payer,
            payee,
            amount_per_interval,
            interval_seconds,
            total_paid: 0,
            max_total,
            status: StreamStatus::Active,
            created_at: now,
            last_payment: None,
            next_payment: now + ChronoDuration::seconds(interval_seconds as i64),
            failure_count: 0,
            channel_id: None,
        }
    }

    /// Check if payment is due
    pub fn is_payment_due(&self) -> bool {
        self.status == StreamStatus::Active && Utc::now() >= self.next_payment
    }

    /// Check if stream has reached max payment
    pub fn is_max_reached(&self) -> bool {
        self.max_total > 0 && self.total_paid >= self.max_total
    }

    /// Record a successful payment
    pub fn record_payment(&mut self, amount: u64) {
        self.total_paid += amount;
        self.last_payment = Some(Utc::now());
        self.next_payment = Utc::now() + ChronoDuration::seconds(self.interval_seconds as i64);
        self.failure_count = 0;

        if self.is_max_reached() {
            self.status = StreamStatus::Completed;
        }
    }

    /// Record a payment failure
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        // Suspend after 3 consecutive failures
        if self.failure_count >= 3 {
            self.status = StreamStatus::Suspended;
        }
    }
}

/// Configuration for the payment processor
#[derive(Debug, Clone)]
pub struct PaymentProcessorConfig {
    /// Processing interval in seconds
    pub interval_seconds: u64,
    /// Maximum retries per stream per cycle
    pub max_retries: u32,
    /// Batch size for processing
    pub batch_size: usize,
    /// Minimum payment amount to process
    pub min_payment_amount: u64,
}

impl Default for PaymentProcessorConfig {
    fn default() -> Self {
        Self {
            interval_seconds: 300, // 5 minutes
            max_retries: 3,
            batch_size: 100,
            min_payment_amount: 1000, // 0.001 DCHAT
        }
    }
}

/// Statistics for the payment processor
#[derive(Debug, Clone, Default)]
pub struct PaymentProcessorStats {
    /// Total payments processed
    pub total_payments: u64,
    /// Total amount transferred
    pub total_amount: u64,
    /// Failed payments
    pub failed_payments: u64,
    /// Active streams
    pub active_streams: usize,
    /// Suspended streams
    pub suspended_streams: usize,
    /// Last processing time
    pub last_processed: Option<DateTime<Utc>>,
}

/// Background payment processor service
pub struct PaymentProcessor {
    config: PaymentProcessorConfig,
    /// Active payment streams
    streams: Arc<RwLock<HashMap<String, PaymentStream>>>,
    /// Currency chain client for transfers
    currency_chain: Arc<CurrencyChainClient>,
    /// Payment channel manager (optional)
    channel_manager: Option<Arc<PaymentChannelManager>>,
    /// Processing statistics
    stats: Arc<RwLock<PaymentProcessorStats>>,
    /// Shutdown receiver
    shutdown_rx: watch::Receiver<bool>,
    /// Payment receipts (recent history)
    receipts: Arc<RwLock<Vec<PaymentReceipt>>>,
}

impl PaymentProcessor {
    /// Create a new payment processor
    pub fn new(
        config: PaymentProcessorConfig,
        currency_chain: Arc<CurrencyChainClient>,
    ) -> (Self, watch::Sender<bool>) {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        (
            Self {
                config,
                streams: Arc::new(RwLock::new(HashMap::new())),
                currency_chain,
                channel_manager: None,
                stats: Arc::new(RwLock::new(PaymentProcessorStats::default())),
                shutdown_rx,
                receipts: Arc::new(RwLock::new(Vec::new())),
            },
            shutdown_tx,
        )
    }

    /// Create with payment channel manager
    pub fn with_channel_manager(
        config: PaymentProcessorConfig,
        currency_chain: Arc<CurrencyChainClient>,
        channel_manager: Arc<PaymentChannelManager>,
    ) -> (Self, watch::Sender<bool>) {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        (
            Self {
                config,
                streams: Arc::new(RwLock::new(HashMap::new())),
                currency_chain,
                channel_manager: Some(channel_manager),
                stats: Arc::new(RwLock::new(PaymentProcessorStats::default())),
                shutdown_rx,
                receipts: Arc::new(RwLock::new(Vec::new())),
            },
            shutdown_tx,
        )
    }

    /// Start the payment processing loop
    pub async fn run(&mut self) {
        let mut interval_timer = interval(Duration::from_secs(self.config.interval_seconds));

        tracing::info!(
            "Payment processor started (interval: {}s, batch_size: {})",
            self.config.interval_seconds,
            self.config.batch_size
        );

        loop {
            tokio::select! {
                _ = interval_timer.tick() => {
                    if let Err(e) = self.process_pending_payments().await {
                        tracing::error!("Payment processing error: {}", e);
                    }
                }
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        tracing::info!("Payment processor shutting down");
                        break;
                    }
                }
            }
        }
    }

    /// Process all pending payments
    async fn process_pending_payments(&self) -> Result<(), PaymentProcessorError> {
        let streams = self.streams.read().await;
        
        // Get streams due for payment
        let due_streams: Vec<String> = streams
            .iter()
            .filter(|(_, stream)| stream.is_payment_due())
            .take(self.config.batch_size)
            .map(|(id, _)| id.clone())
            .collect();

        drop(streams);

        if due_streams.is_empty() {
            return Ok(());
        }

        tracing::info!("Processing {} pending micropayments", due_streams.len());

        for stream_id in due_streams {
            match self.process_stream_payment(&stream_id).await {
                Ok(receipt) => {
                    tracing::debug!(
                        "Processed payment: stream={}, amount={}, tx={}",
                        receipt.stream_id, receipt.amount, receipt.tx_id
                    );
                }
                Err(PaymentProcessorError::InsufficientFunds) => {
                    tracing::warn!("Insufficient funds for stream {}, suspending", stream_id);
                    self.suspend_stream(&stream_id).await?;
                }
                Err(e) => {
                    tracing::error!("Failed to process stream {}: {}", stream_id, e);
                    // Record failure but continue with other streams
                    self.record_stream_failure(&stream_id).await?;
                }
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.last_processed = Some(Utc::now());
            
            let streams = self.streams.read().await;
            stats.active_streams = streams.values()
                .filter(|s| s.status == StreamStatus::Active)
                .count();
            stats.suspended_streams = streams.values()
                .filter(|s| s.status == StreamStatus::Suspended)
                .count();
        }

        Ok(())
    }

    /// Process a single stream payment
    async fn process_stream_payment(
        &self,
        stream_id: &str,
    ) -> Result<PaymentReceipt, PaymentProcessorError> {
        let streams = self.streams.read().await;
        let stream = streams
            .get(stream_id)
            .ok_or_else(|| PaymentProcessorError::StreamNotFound(stream_id.to_string()))?
            .clone();
        drop(streams);

        if stream.status != StreamStatus::Active {
            return Err(PaymentProcessorError::StreamSuspended(stream_id.to_string()));
        }

        let amount = stream.amount_per_interval;

        // Try to process through payment channel first (faster, lower fees)
        let tx_id = if let (Some(ref channel_manager), Some(ref channel_id)) = 
            (&self.channel_manager, &stream.channel_id) 
        {
            // Use payment channel for off-chain payment
            channel_manager
                .process_payment(channel_id, amount)
                .map_err(|e| PaymentProcessorError::ChannelError(e.to_string()))?
        } else {
            // Fall back to on-chain transfer
            self.currency_chain
                .transfer(&stream.payer, &stream.payee, amount)
                .map_err(|e| PaymentProcessorError::CurrencyChainError(e.to_string()))?
                .to_string()
        };

        // Create receipt
        let receipt = PaymentReceipt {
            receipt_id: Uuid::new_v4(),
            stream_id: stream_id.to_string(),
            payer: stream.payer.clone(),
            payee: stream.payee.clone(),
            amount,
            tx_id,
            timestamp: Utc::now(),
        };

        // Update stream
        {
            let mut streams = self.streams.write().await;
            if let Some(stream_mut) = streams.get_mut(stream_id) {
                stream_mut.record_payment(amount);
            }
        }

        // Store receipt
        {
            let mut receipts = self.receipts.write().await;
            receipts.push(receipt.clone());
            // Keep only last 1000 receipts
            let len = receipts.len();
            if len > 1000 {
                receipts.drain(0..len - 1000);
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_payments += 1;
            stats.total_amount += amount;
        }

        Ok(receipt)
    }

    /// Create a new payment stream
    pub async fn create_stream(
        &self,
        payer: UserId,
        payee: UserId,
        amount_per_interval: u64,
        interval_seconds: u64,
        max_total: u64,
        channel_id: Option<String>,
    ) -> Result<String, PaymentProcessorError> {
        let mut stream = PaymentStream::new(
            payer,
            payee,
            amount_per_interval,
            interval_seconds,
            max_total,
        );
        stream.channel_id = channel_id;

        let stream_id = stream.stream_id.clone();

        let mut streams = self.streams.write().await;
        streams.insert(stream_id.clone(), stream);

        tracing::info!(
            "Created payment stream {}: {} tokens every {}s",
            stream_id, amount_per_interval, interval_seconds
        );

        Ok(stream_id)
    }

    /// Suspend a payment stream
    pub async fn suspend_stream(&self, stream_id: &str) -> Result<(), PaymentProcessorError> {
        let mut streams = self.streams.write().await;
        let stream = streams
            .get_mut(stream_id)
            .ok_or_else(|| PaymentProcessorError::StreamNotFound(stream_id.to_string()))?;

        stream.status = StreamStatus::Suspended;
        
        let mut stats = self.stats.write().await;
        stats.failed_payments += 1;

        Ok(())
    }

    /// Resume a suspended stream
    pub async fn resume_stream(&self, stream_id: &str) -> Result<(), PaymentProcessorError> {
        let mut streams = self.streams.write().await;
        let stream = streams
            .get_mut(stream_id)
            .ok_or_else(|| PaymentProcessorError::StreamNotFound(stream_id.to_string()))?;

        if stream.status != StreamStatus::Suspended {
            return Err(PaymentProcessorError::InternalError(
                "Stream is not suspended".to_string()
            ));
        }

        stream.status = StreamStatus::Active;
        stream.failure_count = 0;
        stream.next_payment = Utc::now(); // Trigger immediate payment

        Ok(())
    }

    /// Cancel a payment stream
    pub async fn cancel_stream(&self, stream_id: &str) -> Result<(), PaymentProcessorError> {
        let mut streams = self.streams.write().await;
        let stream = streams
            .get_mut(stream_id)
            .ok_or_else(|| PaymentProcessorError::StreamNotFound(stream_id.to_string()))?;

        stream.status = StreamStatus::Cancelled;

        Ok(())
    }

    /// Record a stream failure (but don't suspend yet)
    async fn record_stream_failure(&self, stream_id: &str) -> Result<(), PaymentProcessorError> {
        let mut streams = self.streams.write().await;
        if let Some(stream) = streams.get_mut(stream_id) {
            stream.record_failure();
        }
        Ok(())
    }

    /// Get stream by ID
    pub async fn get_stream(&self, stream_id: &str) -> Option<PaymentStream> {
        let streams = self.streams.read().await;
        streams.get(stream_id).cloned()
    }

    /// Get all streams for a payer
    pub async fn get_streams_by_payer(&self, payer: &UserId) -> Vec<PaymentStream> {
        let streams = self.streams.read().await;
        streams
            .values()
            .filter(|s| s.payer == *payer)
            .cloned()
            .collect()
    }

    /// Get streams due for payment
    pub async fn get_streams_due(&self) -> Vec<String> {
        let streams = self.streams.read().await;
        streams
            .iter()
            .filter(|(_, stream)| stream.is_payment_due())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get processor statistics
    pub async fn get_stats(&self) -> PaymentProcessorStats {
        self.stats.read().await.clone()
    }

    /// Get recent receipts
    pub async fn get_recent_receipts(&self, limit: usize) -> Vec<PaymentReceipt> {
        let receipts = self.receipts.read().await;
        receipts.iter().rev().take(limit).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_stream_creation() {
        let payer = UserId::new();
        let payee = UserId::new();
        
        let stream = PaymentStream::new(
            payer.clone(),
            payee.clone(),
            1000,
            60,
            10000,
        );

        assert_eq!(stream.payer, payer);
        assert_eq!(stream.payee, payee);
        assert_eq!(stream.amount_per_interval, 1000);
        assert_eq!(stream.interval_seconds, 60);
        assert_eq!(stream.status, StreamStatus::Active);
        assert_eq!(stream.total_paid, 0);
    }

    #[test]
    fn test_payment_recording() {
        let mut stream = PaymentStream::new(
            UserId::new(),
            UserId::new(),
            1000,
            60,
            5000,
        );

        stream.record_payment(1000);
        assert_eq!(stream.total_paid, 1000);
        assert_eq!(stream.failure_count, 0);
        assert!(stream.last_payment.is_some());

        // Record 4 more payments to reach max
        for _ in 0..4 {
            stream.record_payment(1000);
        }

        assert_eq!(stream.total_paid, 5000);
        assert_eq!(stream.status, StreamStatus::Completed);
    }

    #[test]
    fn test_failure_suspension() {
        let mut stream = PaymentStream::new(
            UserId::new(),
            UserId::new(),
            1000,
            60,
            0, // No max
        );

        // First two failures don't suspend
        stream.record_failure();
        assert_eq!(stream.status, StreamStatus::Active);
        stream.record_failure();
        assert_eq!(stream.status, StreamStatus::Active);

        // Third failure suspends
        stream.record_failure();
        assert_eq!(stream.status, StreamStatus::Suspended);
    }

    #[test]
    fn test_config_default() {
        let config = PaymentProcessorConfig::default();
        assert_eq!(config.interval_seconds, 300);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.batch_size, 100);
    }
}
