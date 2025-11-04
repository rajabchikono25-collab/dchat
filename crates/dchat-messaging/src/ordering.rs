//! Message ordering via blockchain sequence numbers

use dchat_core::types::MessageId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Sequence number for message ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SequenceNumber(pub u64);

impl SequenceNumber {
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

/// Message ordering tracker
pub struct MessageOrder {
    /// Next expected sequence per conversation
    expected_sequences: HashMap<String, SequenceNumber>,

    /// Out-of-order messages waiting for gaps to fill
    pending_messages: HashMap<String, Vec<(SequenceNumber, MessageId)>>,
}

impl MessageOrder {
    pub fn new() -> Self {
        Self {
            expected_sequences: HashMap::new(),
            pending_messages: HashMap::new(),
        }
    }

    /// Register a message with its sequence number
    /// Returns true if message is in order, false if out of order
    pub fn register_message(
        &mut self,
        conversation_id: String,
        sequence: SequenceNumber,
        message_id: MessageId,
    ) -> bool {
        let expected = self
            .expected_sequences
            .entry(conversation_id.clone())
            .or_insert(SequenceNumber(0));

        if sequence == *expected {
            // Message is in order
            *expected = expected.next();

            // Check if we can deliver pending messages
            self.deliver_pending(&conversation_id);

            true
        } else if sequence > *expected {
            // Message is ahead, queue it
            self.pending_messages
                .entry(conversation_id)
                .or_default()
                .push((sequence, message_id));

            false
        } else {
            // Duplicate or very old message, ignore
            false
        }
    }

    /// Attempt to deliver pending messages in order
    fn deliver_pending(&mut self, conversation_id: &str) {
        if let Some(pending) = self.pending_messages.get_mut(conversation_id) {
            pending.sort_by_key(|(seq, _)| *seq);

            let expected = self
                .expected_sequences
                .get_mut(conversation_id)
                .expect("Expected sequence must exist");

            // Deliver all consecutive messages
            pending.retain(|(seq, _)| {
                if *seq == *expected {
                    *expected = expected.next();
                    false // Remove from pending
                } else {
                    true // Keep in pending
                }
            });
        }
    }

    /// Get the next expected sequence for a conversation
    pub fn next_sequence(&mut self, conversation_id: String) -> SequenceNumber {
        let seq = self
            .expected_sequences
            .entry(conversation_id)
            .or_insert(SequenceNumber(0));

        let current = *seq;
        *seq = seq.next();
        current
    }

    /// Check for gaps in sequence (missing messages)
    pub fn check_gaps(&self, conversation_id: &str) -> Vec<SequenceNumber> {
        if let Some(pending) = self.pending_messages.get(conversation_id) {
            let expected = self
                .expected_sequences
                .get(conversation_id)
                .copied()
                .unwrap_or(SequenceNumber(0));

            let mut gaps = Vec::new();
            let mut sequences: Vec<_> = pending.iter().map(|(seq, _)| *seq).collect();
            sequences.sort();

            let mut current = expected;
            for &seq in &sequences {
                while current < seq {
                    gaps.push(current);
                    current = current.next();
                }
                current = seq.next();
            }

            gaps
        } else {
            Vec::new()
        }
    }

    /// Get pending message count for a conversation
    pub fn pending_count(&self, conversation_id: &str) -> usize {
        self.pending_messages
            .get(conversation_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

impl Default for MessageOrder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_order_messages() {
        let mut order = MessageOrder::new();
        let conv = "conv1".to_string();

        let msg1 = MessageId(uuid::Uuid::new_v4());
        let msg2 = MessageId(uuid::Uuid::new_v4());
        let msg3 = MessageId(uuid::Uuid::new_v4());

        assert!(order.register_message(conv.clone(), SequenceNumber(0), msg1));
        assert!(order.register_message(conv.clone(), SequenceNumber(1), msg2));
        assert!(order.register_message(conv.clone(), SequenceNumber(2), msg3));

        assert_eq!(order.pending_count(&conv), 0);
    }

    #[test]
    fn test_out_of_order_messages() {
        let mut order = MessageOrder::new();
        let conv = "conv1".to_string();

        let msg1 = MessageId(uuid::Uuid::new_v4());
        let msg3 = MessageId(uuid::Uuid::new_v4());
        let msg2 = MessageId(uuid::Uuid::new_v4());

        // Receive in order: 0, 2, 1
        assert!(order.register_message(conv.clone(), SequenceNumber(0), msg1));
        assert!(!order.register_message(conv.clone(), SequenceNumber(2), msg3)); // Out of order
        assert_eq!(order.pending_count(&conv), 1);

        // Now deliver msg with seq 1 - should trigger delivery of msg3 too
        assert!(order.register_message(conv.clone(), SequenceNumber(1), msg2));
        assert_eq!(order.pending_count(&conv), 0);
    }

    #[test]
    fn test_gap_detection() {
        let mut order = MessageOrder::new();
        let conv = "conv1".to_string();

        let msg1 = MessageId(uuid::Uuid::new_v4());
        let msg4 = MessageId(uuid::Uuid::new_v4());

        order.register_message(conv.clone(), SequenceNumber(0), msg1);
        order.register_message(conv.clone(), SequenceNumber(3), msg4);

        let gaps = order.check_gaps(&conv);
        assert_eq!(gaps, vec![SequenceNumber(1), SequenceNumber(2)]);
    }
}
