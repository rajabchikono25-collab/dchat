/// dchat SDK for Flutter/Dart
///
/// Provides blockchain-integrated user management, messaging, and channel
/// operations for decentralized chat applications.
///
/// Example:
/// ```dart
/// import 'package:dchat_sdk/dchat.dart';
///
/// void main() async {
///   final client = BlockchainClient(
///     rpcUrl: 'http://localhost:8545',
///   );
///
///   // Register user
///   final userResponse = await client.registerUser('alice');
///   print('User registered: ${userResponse.userId}');
///   print('On-chain confirmed: ${userResponse.onChainConfirmed}');
///
///   // Send message
///   final msgResponse = await client.sendDirectMessage(
///     senderId: userResponse.userId,
///     recipientId: 'recipient-id',
///     content: 'Hello, dchat!',
///   );
/// }
/// ```

library dchat;

export 'src/blockchain/client.dart';
export 'src/blockchain/transaction.dart';
export 'src/user/manager.dart';
export 'src/user/models.dart';
export 'src/crypto/keypair.dart';
export 'src/messaging/crypto.dart';
export 'src/messaging/dht.dart';
export 'src/messaging/peer_manager.dart';
export 'src/messaging/proof_of_delivery.dart';
export 'src/messaging/message_manager.dart';
export 'src/messaging/websocket_client.dart';
export 'src/messaging/http_client.dart';
