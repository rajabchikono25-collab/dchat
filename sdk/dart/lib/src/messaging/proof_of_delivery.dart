// Proof of delivery tracking for messages
library;

import 'websocket_client.dart';

/// Delivery proof states
enum DeliveryStatus {
  pending,
  delivered,
  read,
  failed,
}

/// Proof of delivery receipt
class DeliveryProof {
  final String messageId;
  final String recipientId;
  final String senderPublicKey;
  final DateTime timestamp;
  final DeliveryStatus status;
  final String? signature;
  final String? relayNodeId;
  final int? blockHeight;

  DeliveryProof({
    required this.messageId,
    required this.recipientId,
    required this.senderPublicKey,
    required this.status,
    this.signature,
    this.relayNodeId,
    this.blockHeight,
  }) : timestamp = DateTime.now();

  /// Verify delivery proof signature
  bool verifySignature(String publicKey) {
    if (signature == null) return false;
    // In production, verify ED25519 signature
    // This is a placeholder
    return signature!.isNotEmpty;
  }

  Map<String, dynamic> toJson() => {
        'messageId': messageId,
        'recipientId': recipientId,
        'senderPublicKey': senderPublicKey,
        'timestamp': timestamp.toIso8601String(),
        'status': status.toString(),
        'signature': signature,
        'relayNodeId': relayNodeId,
        'blockHeight': blockHeight,
      };

  factory DeliveryProof.fromJson(Map<String, dynamic> json) {
    return DeliveryProof(
      messageId: json['messageId'],
      recipientId: json['recipientId'],
      senderPublicKey: json['senderPublicKey'],
      status: DeliveryStatus.values.firstWhere(
        (e) => e.toString() == json['status'],
      ),
      signature: json['signature'],
      relayNodeId: json['relayNodeId'],
      blockHeight: json['blockHeight'],
    );
  }
}

/// Proof of delivery tracker
class ProofOfDeliveryTracker {
  final Map<String, DeliveryProof> proofs = {};
  final Map<String, DateTime> pendingMessages = {};
  static const Duration proofTimeout = Duration(minutes: 30);
  WebSocketClient? _wsClient;

  /// Connect to WebSocket for real-time delivery updates
  void connectWebSocket(WebSocketClient wsClient) {
    _wsClient = wsClient;
    
    // Listen for delivery proof messages from relay
    _wsClient?.onMessage((data) {
      final messageType = data['type'] as String?;
      
      if (messageType == 'delivery_proof') {
        final proof = DeliveryProof.fromJson(data);
        recordProof(proof);
      } else if (messageType == 'read_receipt') {
        final messageId = data['messageId'] as String;
        final existingProof = proofs[messageId];
        
        if (existingProof != null) {
          // Update status to read
          proofs[messageId] = DeliveryProof(
            messageId: messageId,
            recipientId: existingProof.recipientId,
            senderPublicKey: existingProof.senderPublicKey,
            status: DeliveryStatus.read,
            signature: existingProof.signature,
            relayNodeId: existingProof.relayNodeId,
            blockHeight: existingProof.blockHeight,
          );
        }
      }
    });
  }

  /// Disconnect WebSocket
  void disconnectWebSocket() {
    _wsClient = null;
  }

  /// Request delivery status from relay via WebSocket
  Future<void> requestDeliveryStatus(String messageId) async {
    if (_wsClient == null || !_wsClient!.isConnected) {
      throw StateError('WebSocket not connected');
    }
    
    await _wsClient!.send({
      'type': 'request_delivery_status',
      'messageId': messageId,
    });
  }

  /// Record message as pending delivery
  void markPending(String messageId, String recipientId) {
    pendingMessages[messageId] = DateTime.now();
  }

  /// Record delivery proof
  void recordProof(DeliveryProof proof) {
    proofs[proof.messageId] = proof;
    pendingMessages.remove(proof.messageId);
  }

  /// Get proof for message
  DeliveryProof? getProof(String messageId) => proofs[messageId];

  /// Check if message was delivered
  bool isDelivered(String messageId) {
    final proof = proofs[messageId];
    return proof != null &&
        (proof.status == DeliveryStatus.delivered ||
            proof.status == DeliveryStatus.read);
  }

  /// Check if message was read
  bool isRead(String messageId) {
    final proof = proofs[messageId];
    return proof != null && proof.status == DeliveryStatus.read;
  }

  /// Get pending messages
  List<String> getPendingMessages() {
    final now = DateTime.now();
    final pending = <String>[];

    pendingMessages.removeWhere((id, timestamp) {
      if (now.difference(timestamp).inSeconds > proofTimeout.inSeconds) {
        // Mark as failed
        proofs[id] = DeliveryProof(
          messageId: id,
          recipientId: 'unknown',
          senderPublicKey: 'unknown',
          status: DeliveryStatus.failed,
        );
        return true;
      }
      pending.add(id);
      return false;
    });

    return pending;
  }

  /// Get delivery statistics
  Map<String, dynamic> getStats() {
    int delivered = 0;
    int read = 0;
    int failed = 0;

    for (final proof in proofs.values) {
      switch (proof.status) {
        case DeliveryStatus.delivered:
          delivered++;
          break;
        case DeliveryStatus.read:
          read++;
          break;
        case DeliveryStatus.failed:
          failed++;
          break;
        default:
          break;
      }
    }

    return {
      'total': proofs.length,
      'delivered': delivered,
      'read': read,
      'failed': failed,
      'pending': getPendingMessages().length,
      'successRate': proofs.isEmpty
          ? 0.0
          : ((delivered + read) / proofs.length * 100).toStringAsFixed(2),
    };
  }

  /// Clear old proofs
  void prune({Duration retention = const Duration(days: 7)}) {
    final cutoff = DateTime.now().subtract(retention);
    proofs.removeWhere((_, proof) => proof.timestamp.isBefore(cutoff));
  }
}
