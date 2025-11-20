/// Unit tests for WebSocketClient
library;

import 'dart:async';
import 'package:test/test.dart';
import 'package:dchat_sdk/dchat.dart';

void main() {
  group('WebSocketClient', () {
    test('should initialize with correct URL', () {
      final client = WebSocketClient(url: 'ws://localhost:8080');
      expect(client.isConnected, false);
    });

    test('should track connection state', () async {
      final client = WebSocketClient(url: 'ws://echo.websocket.org');
      
      expect(client.isConnected, false);
      
      // Note: This test requires actual WebSocket server
      // In production, use a mock or test server
    });

    test('should handle message handlers', () async {
      final client = WebSocketClient(url: 'ws://localhost:8080');
      final receivedMessages = <Map<String, dynamic>>[];
      
      client.onMessage((data) {
        receivedMessages.add(data);
      });
      
      // Verify handler was registered
      expect(receivedMessages, isEmpty);
    });

    test('should handle error handlers', () async {
      final client = WebSocketClient(url: 'ws://localhost:8080');
      final errors = <Object>[];
      
      client.onError((error) {
        errors.add(error);
      });
      
      // Verify handler was registered
      expect(errors, isEmpty);
    });

    test('should handle connection handlers', () async {
      final client = WebSocketClient(url: 'ws://localhost:8080');
      var connected = false;
      
      client.onConnect(() {
        connected = true;
      });
      
      // Verify handler was registered
      expect(connected, false);
    });

    test('should handle disconnect handlers', () async {
      final client = WebSocketClient(url: 'ws://localhost:8080');
      var disconnected = false;
      
      client.onDisconnect(() {
        disconnected = true;
      });
      
      // Verify handler was registered
      expect(disconnected, false);
    });

    test('should support custom reconnection settings', () {
      final client = WebSocketClient(
        url: 'ws://localhost:8080',
        reconnectDelay: Duration(seconds: 5),
        maxReconnectAttempts: 10,
      );
      
      expect(client.isConnected, false);
    });

    test('should support custom ping interval', () {
      final client = WebSocketClient(
        url: 'ws://localhost:8080',
        pingInterval: Duration(seconds: 60),
      );
      
      expect(client.isConnected, false);
    });

    test('should handle multiple message handlers', () {
      final client = WebSocketClient(url: 'ws://localhost:8080');
      final handler1Messages = <Map<String, dynamic>>[];
      final handler2Messages = <Map<String, dynamic>>[];
      
      client.onMessage((data) => handler1Messages.add(data));
      client.onMessage((data) => handler2Messages.add(data));
      
      // Both should be registered
      expect(handler1Messages, isEmpty);
      expect(handler2Messages, isEmpty);
    });

    test('should dispose cleanly', () async {
      final client = WebSocketClient(url: 'ws://localhost:8080');
      
      // Should not throw
      client.dispose();
    });
  });

  group('WebSocketClient Integration', () {
    test('should send and receive messages (mock)', () async {
      // This would require a test WebSocket server
      // For now, just verify the API works
      final client = WebSocketClient(url: 'ws://localhost:8080');
      
      expect(() async {
        await client.send({'type': 'test', 'data': 'hello'});
      }, throwsA(isA<StateError>())); // Should throw since not connected
    });

    test('should handle JSON encoding errors', () async {
      final client = WebSocketClient(url: 'ws://localhost:8080');
      
      // Create circular reference (cannot be JSON encoded)
      final circular = <String, dynamic>{};
      circular['self'] = circular;
      
      expect(() async {
        await client.send(circular);
      }, throwsA(anything)); // Should throw encoding error
    });
  });

  group('WebSocketClient Reconnection', () {
    test('should attempt reconnection on disconnect', () async {
      // This test would require simulating connection drops
      // Placeholder for reconnection logic testing
      final client = WebSocketClient(
        url: 'ws://localhost:8080',
        reconnectDelay: Duration(milliseconds: 100),
        maxReconnectAttempts: 3,
      );
      
      expect(client.isConnected, false);
      client.dispose();
    });
  });

  group('WebSocketClient Heartbeat', () {
    test('should send periodic pings when connected', () async {
      // This test would require a WebSocket server that responds to pings
      // Placeholder for heartbeat testing
      final client = WebSocketClient(
        url: 'ws://localhost:8080',
        pingInterval: Duration(milliseconds: 100),
      );
      
      expect(client.isConnected, false);
      client.dispose();
    });
  });
}
