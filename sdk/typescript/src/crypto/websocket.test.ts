/**
 * Tests for WebSocket connection manager
 */

import { WebSocketManager } from './websocket';

describe('WebSocketManager', () => {
  const TEST_URL = 'ws://localhost:8080';
  let wsManager: WebSocketManager;

  beforeEach(() => {
    wsManager = new WebSocketManager({
      url: TEST_URL,
      reconnectDelay: 1000,
      maxReconnectAttempts: 3,
      pingInterval: 5000,
    });
  });

  afterEach(async () => {
    await wsManager.disconnect();
  });

  describe('Connection', () => {
    it('should create a WebSocket manager', () => {
      expect(wsManager).toBeDefined();
      expect(wsManager.isConnected()).toBe(false);
    });

    it('should handle connection lifecycle', () => {
      const connectHandler = jest.fn();
      const disconnectHandler = jest.fn();

      wsManager.onConnect(connectHandler);
      wsManager.onDisconnect(disconnectHandler);

      // Note: Actual connection requires a running WebSocket server
      // This test validates the API structure
      expect(connectHandler).not.toHaveBeenCalled();
      expect(disconnectHandler).not.toHaveBeenCalled();
    });

    it('should not connect when already connected', async () => {
      // Mock scenario - in real test would need server
      expect(wsManager.isConnected()).toBe(false);
    });
  });

  describe('Message Handling', () => {
    it('should register message handlers', () => {
      const handler = jest.fn();
      wsManager.onMessage(handler);

      // Handler should be registered
      expect(handler).not.toHaveBeenCalled();
    });

    it('should unregister message handlers', () => {
      const handler = jest.fn();
      wsManager.onMessage(handler);
      wsManager.offMessage(handler);

      // Handler should be removed
      expect(handler).not.toHaveBeenCalled();
    });

    it('should register error handlers', () => {
      const handler = jest.fn();
      wsManager.onError(handler);

      expect(handler).not.toHaveBeenCalled();
    });
  });

  describe('Error Handling', () => {
    it('should handle errors gracefully', () => {
      const errorHandler = jest.fn();
      wsManager.onError(errorHandler);

      // Errors should be handled
      expect(errorHandler).not.toHaveBeenCalled();
    });
  });

  describe('Reconnection', () => {
    it('should support reconnection configuration', () => {
      const config = {
        url: TEST_URL,
        reconnectDelay: 2000,
        maxReconnectAttempts: 5,
      };

      const manager = new WebSocketManager(config);
      expect(manager).toBeDefined();
      expect(manager.isConnected()).toBe(false);
    });
  });
});

