/// WebSocket client for relay connections
library;

import 'dart:async';
import 'dart:convert';
import 'package:web_socket_channel/web_socket_channel.dart';

/// WebSocket connection state
enum ConnectionState {
  disconnected,
  connecting,
  connected,
  reconnecting,
  failed,
}

/// WebSocket message handler
typedef MessageHandler = void Function(Map<String, dynamic> message);

/// WebSocket error handler
typedef ErrorHandler = void Function(Object error);

/// WebSocket connection handler
typedef ConnectionHandler = void Function();

/// WebSocket client for relay connections
class WebSocketClient {
  final String url;
  final Duration reconnectDelay;
  final int maxReconnectAttempts;
  final Duration pingInterval;

  WebSocketChannel? _channel;
  StreamSubscription? _subscription;
  Timer? _reconnectTimer;
  Timer? _pingTimer;
  
  ConnectionState _state = ConnectionState.disconnected;
  int _reconnectAttempts = 0;
  bool _shouldReconnect = true;

  final List<MessageHandler> _messageHandlers = [];
  final List<ErrorHandler> _errorHandlers = [];
  final List<ConnectionHandler> _connectHandlers = [];
  final List<ConnectionHandler> _disconnectHandlers = [];

  WebSocketClient({
    required this.url,
    this.reconnectDelay = const Duration(seconds: 5),
    this.maxReconnectAttempts = 10,
    this.pingInterval = const Duration(seconds: 30),
  });

  /// Get current connection state
  ConnectionState get state => _state;

  /// Check if connected
  bool get isConnected => _state == ConnectionState.connected;

  /// Connect to the WebSocket server
  Future<void> connect() async {
    if (_state == ConnectionState.connected) {
      return; // Already connected
    }

    if (_state == ConnectionState.connecting) {
      return; // Connection in progress
    }

    _state = ConnectionState.connecting;
    _shouldReconnect = true;

    try {
      _channel = WebSocketChannel.connect(Uri.parse(url));
      
      // Listen for messages
      _subscription = _channel!.stream.listen(
        (data) {
          try {
            final message = jsonDecode(data as String) as Map<String, dynamic>;
            _notifyMessage(message);
          } catch (e) {
            _notifyError(Exception('Failed to parse message: $e'));
          }
        },
        onError: (error) {
          _notifyError(error);
          _handleDisconnect();
        },
        onDone: () {
          _handleDisconnect();
        },
      );

      _state = ConnectionState.connected;
      _reconnectAttempts = 0;
      _startPing();
      _notifyConnect();
    } catch (e) {
      _state = ConnectionState.failed;
      _notifyError(e);
      _scheduleReconnect();
      rethrow;
    }
  }

  /// Disconnect from the WebSocket server
  Future<void> disconnect() async {
    _shouldReconnect = false;
    _cancelReconnectTimer();
    _stopPing();

    await _subscription?.cancel();
    await _channel?.sink.close();

    _subscription = null;
    _channel = null;
    _state = ConnectionState.disconnected;
  }

  /// Send a message to the server
  Future<void> send(Map<String, dynamic> message) async {
    if (!isConnected) {
      throw StateError('Not connected to WebSocket server');
    }

    try {
      final data = jsonEncode(message);
      _channel?.sink.add(data);
    } catch (e) {
      _notifyError(e);
      rethrow;
    }
  }

  /// Register a message handler
  void onMessage(MessageHandler handler) {
    _messageHandlers.add(handler);
  }

  /// Unregister a message handler
  void offMessage(MessageHandler handler) {
    _messageHandlers.remove(handler);
  }

  /// Register an error handler
  void onError(ErrorHandler handler) {
    _errorHandlers.add(handler);
  }

  /// Unregister an error handler
  void offError(ErrorHandler handler) {
    _errorHandlers.remove(handler);
  }

  /// Register a connect handler
  void onConnect(ConnectionHandler handler) {
    _connectHandlers.add(handler);
  }

  /// Unregister a connect handler
  void offConnect(ConnectionHandler handler) {
    _connectHandlers.remove(handler);
  }

  /// Register a disconnect handler
  void onDisconnect(ConnectionHandler handler) {
    _disconnectHandlers.add(handler);
  }

  /// Unregister a disconnect handler
  void offDisconnect(ConnectionHandler handler) {
    _disconnectHandlers.remove(handler);
  }

  /// Handle disconnection
  void _handleDisconnect() {
    if (_state == ConnectionState.disconnected) {
      return;
    }

    _state = ConnectionState.disconnected;
    _stopPing();
    _notifyDisconnect();

    if (_shouldReconnect) {
      _scheduleReconnect();
    }
  }

  /// Schedule a reconnection attempt
  void _scheduleReconnect() {
    if (_reconnectAttempts >= maxReconnectAttempts) {
      _state = ConnectionState.failed;
      _notifyError(Exception('Max reconnection attempts reached'));
      return;
    }

    _cancelReconnectTimer();
    _reconnectAttempts++;
    _state = ConnectionState.reconnecting;

    _reconnectTimer = Timer(reconnectDelay, () {
      connect().catchError((error) {
        _notifyError(error);
      });
    });
  }

  /// Cancel reconnect timer
  void _cancelReconnectTimer() {
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
  }

  /// Start ping/pong heartbeat
  void _startPing() {
    _stopPing();

    _pingTimer = Timer.periodic(pingInterval, (_) {
      if (isConnected) {
        try {
          send({'type': 'ping', 'timestamp': DateTime.now().toIso8601String()});
        } catch (e) {
          _notifyError(e);
        }
      }
    });
  }

  /// Stop ping/pong heartbeat
  void _stopPing() {
    _pingTimer?.cancel();
    _pingTimer = null;
  }

  /// Notify message handlers
  void _notifyMessage(Map<String, dynamic> message) {
    for (final handler in _messageHandlers) {
      try {
        handler(message);
      } catch (e) {
        _notifyError(e);
      }
    }
  }

  /// Notify error handlers
  void _notifyError(Object error) {
    for (final handler in _errorHandlers) {
      try {
        handler(error);
      } catch (e) {
        // Log error in error handler
        print('Error in error handler: $e');
      }
    }
  }

  /// Notify connect handlers
  void _notifyConnect() {
    for (final handler in _connectHandlers) {
      try {
        handler();
      } catch (e) {
        _notifyError(e);
      }
    }
  }

  /// Notify disconnect handlers
  void _notifyDisconnect() {
    for (final handler in _disconnectHandlers) {
      try {
        handler();
      } catch (e) {
        print('Error in disconnect handler: $e');
      }
    }
  }

  /// Dispose resources
  void dispose() {
    disconnect();
    _messageHandlers.clear();
    _errorHandlers.clear();
    _connectHandlers.clear();
    _disconnectHandlers.clear();
  }
}
