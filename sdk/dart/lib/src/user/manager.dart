/// User management for creating users and handling profiles
library;

import 'package:uuid/uuid.dart';
import '../blockchain/client.dart';
import '../crypto/keypair.dart';
import '../messaging/http_client.dart';
import '../messaging/websocket_client.dart';
import 'models.dart';

/// User manager for user operations
class UserManager {
  final BlockchainClient blockchain;
  final String baseUrl;
  final Uuid _uuid = const Uuid();
  late final HttpClient _httpClient;
  WebSocketClient? _wsClient;

  UserManager({
    required this.blockchain,
    required this.baseUrl,
  }) {
    _httpClient = HttpClient(baseUrl: baseUrl);
  }

  /// Create a new user with blockchain registration
  Future<CreateUserResponse> createUser(String username) async {
    // Generate unique user ID
    final userId = _uuid.v4();

    // Generate Ed25519 key pair
    final keyPair = KeyPair.generate();

    // Submit blockchain transaction
    final txId = await blockchain.registerUser(
      userId: userId,
      username: username,
      publicKey: keyPair.publicKeyHex,
    );

    // Wait for blockchain confirmation
    final receipt = await blockchain.waitForConfirmation(txId);
    final onChainConfirmed = receipt.success;

    // Return response with actual blockchain status
    return CreateUserResponse(
      userId: userId,
      username: username,
      publicKey: keyPair.publicKeyHex,
      privateKey: keyPair.privateKeyHex,
      createdAt: DateTime.now().toUtc().toIso8601String(),
      onChainConfirmed: onChainConfirmed,
      txId: txId,
    );
  }

  /// Get user profile by user ID
  /// Queries HTTP API for user registration and profile data
  Future<UserProfile?> getUserProfile(String userId) async {
    try {
      final profileData = await _httpClient.get('/api/users/$userId');
      
      if (profileData != null) {
        return UserProfile(
          userId: userId,
          username: profileData['username'] as String,
          publicKey: profileData['publicKey'] as String,
          createdAt: profileData['createdAt'] as String,
          reputation: profileData['reputation'] as int? ?? 0,
          onChainConfirmed: profileData['onChainConfirmed'] as bool? ?? false,
        );
      }
      
      return null;
    } catch (e) {
      // Log error and return null if user not found or error occurred
      return null;
    }
  }

  /// Send a direct message
  Future<DirectMessageResponse> sendDirectMessage({
    required String senderId,
    required String recipientId,
    required String content,
    String? relayNodeId,
  }) async {
    // Generate message ID
    final messageId = _uuid.v4();

    // Hash the content
    final contentHash = hashContent(content);

    // Submit blockchain transaction
    final txId = await blockchain.sendDirectMessage(
      messageId: messageId,
      senderId: senderId,
      recipientId: recipientId,
      contentHash: contentHash,
      payloadSize: content.length,
      relayNodeId: relayNodeId,
    );

    // Wait for confirmation
    final receipt = await blockchain.waitForConfirmation(txId);
    final onChainConfirmed = receipt.success;

    return DirectMessageResponse(
      messageId: messageId,
      senderId: senderId,
      recipientId: recipientId,
      contentHash: contentHash,
      createdAt: DateTime.now().toUtc().toIso8601String(),
      onChainConfirmed: onChainConfirmed,
      txId: txId,
    );
  }

  /// Create a new channel
  Future<CreateChannelResponse> createChannel({
    required String creatorId,
    required String channelName,
    String? description,
  }) async {
    // Generate channel ID
    final channelId = _uuid.v4();

    // Submit blockchain transaction
    final txId = await blockchain.createChannel(
      channelId: channelId,
      name: channelName,
      description: description ?? '',
      creatorId: creatorId,
    );

    // Wait for confirmation
    final receipt = await blockchain.waitForConfirmation(txId);
    final onChainConfirmed = receipt.success;

    return CreateChannelResponse(
      channelId: channelId,
      name: channelName,
      description: description,
      creatorId: creatorId,
      createdAt: DateTime.now().toUtc().toIso8601String(),
      onChainConfirmed: onChainConfirmed,
      txId: txId,
    );
  }

  /// Post a message to a channel
  Future<DirectMessageResponse> postToChannel({
    required String senderId,
    required String channelId,
    required String content,
  }) async {
    // Generate message ID
    final messageId = _uuid.v4();

    // Hash the content
    final contentHash = hashContent(content);

    // Submit blockchain transaction
    final txId = await blockchain.postToChannel(
      messageId: messageId,
      channelId: channelId,
      senderId: senderId,
      contentHash: contentHash,
      payloadSize: content.length,
    );

    // Wait for confirmation
    final receipt = await blockchain.waitForConfirmation(txId);
    final onChainConfirmed = receipt.success;

    return DirectMessageResponse(
      messageId: messageId,
      senderId: senderId,
      recipientId: channelId, // Using channelId as recipient for response
      contentHash: contentHash,
      createdAt: DateTime.now().toUtc().toIso8601String(),
      onChainConfirmed: onChainConfirmed,
      txId: txId,
    );
  }

  /// Get direct messages for a user
  Future<List<DirectMessage>> getDirectMessages({
    required String userId,
    int limit = 50,
  }) async {
    try {
      final response = await _httpClient.get(
        '/api/users/$userId/messages',
        queryParams: {'limit': limit.toString()},
      );

      if (response == null || response['messages'] == null) {
        return [];
      }

      final messagesList = response['messages'] as List;
      return messagesList
          .map((msg) => DirectMessage(
                messageId: msg['messageId'] as String,
                senderId: msg['senderId'] as String,
                recipientId: msg['recipientId'] as String,
                content: msg['content'] as String,
                contentHash: msg['contentHash'] as String,
                createdAt: msg['createdAt'] as String,
                onChainConfirmed: msg['onChainConfirmed'] as bool? ?? false,
              ))
          .toList();
    } catch (e) {
      // Return empty list on error
      return [];
    }
  }

  /// Get channel messages
  Future<List<ChannelMessage>> getChannelMessages({
    required String channelId,
    int limit = 50,
  }) async {
    try {
      final response = await _httpClient.get(
        '/api/channels/$channelId/messages',
        queryParams: {'limit': limit.toString()},
      );

      if (response == null || response['messages'] == null) {
        return [];
      }

      final messagesList = response['messages'] as List;
      return messagesList
          .map((msg) => ChannelMessage(
                messageId: msg['messageId'] as String,
                channelId: msg['channelId'] as String,
                senderId: msg['senderId'] as String,
                content: msg['content'] as String,
                contentHash: msg['contentHash'] as String,
                createdAt: msg['createdAt'] as String,
                onChainConfirmed: msg['onChainConfirmed'] as bool? ?? false,
              ))
          .toList();
    } catch (e) {
      // Return empty list on error
      return [];
    }
  }

  /// Connect to WebSocket relay for real-time messaging
  Future<void> connectToRelay(String relayUrl) async {
    _wsClient = WebSocketClient(url: relayUrl);
    await _wsClient!.connect();
  }

  /// Disconnect from WebSocket relay
  Future<void> disconnectFromRelay() async {
    await _wsClient?.disconnect();
    _wsClient = null;
  }

  /// Check if connected to relay
  bool get isConnectedToRelay => _wsClient?.isConnected ?? false;

  /// Send real-time message via WebSocket
  Future<void> sendRealtimeMessage(Map<String, dynamic> message) async {
    if (_wsClient == null || !_wsClient!.isConnected) {
      throw StateError('Not connected to relay');
    }
    await _wsClient!.send(message);
  }

  /// Register message handler for real-time messages
  void onRealtimeMessage(MessageHandler handler) {
    _wsClient?.onMessage(handler);
  }

  /// Dispose resources
  void dispose() {
    _wsClient?.dispose();
  }
}
