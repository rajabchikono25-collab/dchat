/**
 * WebSocket connection manager for relay connections
 * Handles WebSocket lifecycle, reconnection, and message handling
 */

import WebSocket from 'ws';
import { SdkError } from '../errors';

export interface WebSocketConfig {
  url: string;
  reconnectDelay?: number;
  maxReconnectAttempts?: number;
  pingInterval?: number;
}

export interface MessageHandler {
  (message: any): void;
}

export interface ErrorHandler {
  (error: Error): void;
}

export interface ConnectionHandler {
  (): void;
}

export class WebSocketManager {
  private ws: WebSocket | null = null;
  private config: WebSocketConfig;
  private reconnectAttempts: number = 0;
  private reconnectTimer: NodeJS.Timeout | null = null;
  private pingTimer: NodeJS.Timeout | null = null;
  private isConnecting: boolean = false;
  private shouldReconnect: boolean = true;

  private messageHandlers: Set<MessageHandler> = new Set();
  private errorHandlers: Set<ErrorHandler> = new Set();
  private connectHandlers: Set<ConnectionHandler> = new Set();
  private disconnectHandlers: Set<ConnectionHandler> = new Set();

  constructor(config: WebSocketConfig) {
    this.config = {
      reconnectDelay: 5000,
      maxReconnectAttempts: 10,
      pingInterval: 30000,
      ...config,
    };
  }

  /**
   * Connect to the WebSocket server
   */
  async connect(): Promise<void> {
    if (this.ws?.readyState === WebSocket.OPEN) {
      return; // Already connected
    }

    if (this.isConnecting) {
      return; // Connection in progress
    }

    this.isConnecting = true;
    this.shouldReconnect = true;

    return new Promise((resolve, reject) => {
      try {
        this.ws = new WebSocket(this.config.url);

        this.ws.on('open', () => {
          this.isConnecting = false;
          this.reconnectAttempts = 0;
          this.startPing();
          this.notifyConnect();
          resolve();
        });

        this.ws.on('message', (data: WebSocket.Data) => {
          try {
            const message = JSON.parse(data.toString());
            this.notifyMessage(message);
          } catch (error) {
            this.notifyError(new Error(`Failed to parse message: ${error}`));
          }
        });

        this.ws.on('error', (error: Error) => {
          this.isConnecting = false;
          this.notifyError(error);
          reject(error);
        });

        this.ws.on('close', () => {
          this.isConnecting = false;
          this.stopPing();
          this.notifyDisconnect();

          if (this.shouldReconnect) {
            this.scheduleReconnect();
          }
        });
      } catch (error) {
        this.isConnecting = false;
        reject(error);
      }
    });
  }

  /**
   * Disconnect from the WebSocket server
   */
  async disconnect(): Promise<void> {
    this.shouldReconnect = false;
    this.clearReconnectTimer();
    this.stopPing();

    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  /**
   * Send a message to the server
   */
  async send(message: any): Promise<void> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw SdkError.notConnected();
    }

    const data = JSON.stringify(message);
    return new Promise((resolve, reject) => {
      this.ws!.send(data, (error: Error | undefined) => {
        if (error) {
          reject(error);
        } else {
          resolve();
        }
      });
    });
  }

  /**
   * Check if connected
   */
  isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  /**
   * Register a message handler
   */
  onMessage(handler: MessageHandler): void {
    this.messageHandlers.add(handler);
  }

  /**
   * Unregister a message handler
   */
  offMessage(handler: MessageHandler): void {
    this.messageHandlers.delete(handler);
  }

  /**
   * Register an error handler
   */
  onError(handler: ErrorHandler): void {
    this.errorHandlers.add(handler);
  }

  /**
   * Unregister an error handler
   */
  offError(handler: ErrorHandler): void {
    this.errorHandlers.delete(handler);
  }

  /**
   * Register a connect handler
   */
  onConnect(handler: ConnectionHandler): void {
    this.connectHandlers.add(handler);
  }

  /**
   * Unregister a connect handler
   */
  offConnect(handler: ConnectionHandler): void {
    this.connectHandlers.delete(handler);
  }

  /**
   * Register a disconnect handler
   */
  onDisconnect(handler: ConnectionHandler): void {
    this.disconnectHandlers.add(handler);
  }

  /**
   * Unregister a disconnect handler
   */
  offDisconnect(handler: ConnectionHandler): void {
    this.disconnectHandlers.delete(handler);
  }

  /**
   * Schedule a reconnection attempt
   */
  private scheduleReconnect(): void {
    if (this.reconnectAttempts >= (this.config.maxReconnectAttempts || 10)) {
      this.notifyError(new Error('Max reconnection attempts reached'));
      return;
    }

    this.clearReconnectTimer();
    this.reconnectAttempts++;

    const delay = this.config.reconnectDelay || 5000;
    this.reconnectTimer = setTimeout(() => {
      this.connect().catch((error) => {
        this.notifyError(error);
      });
    }, delay);
  }

  /**
   * Clear reconnect timer
   */
  private clearReconnectTimer(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  /**
   * Start ping/pong heartbeat
   */
  private startPing(): void {
    this.stopPing();

    this.pingTimer = setInterval(() => {
      if (this.ws?.readyState === WebSocket.OPEN) {
        this.ws.ping();
      }
    }, this.config.pingInterval || 30000);
  }

  /**
   * Stop ping/pong heartbeat
   */
  private stopPing(): void {
    if (this.pingTimer) {
      clearInterval(this.pingTimer);
      this.pingTimer = null;
    }
  }

  /**
   * Notify message handlers
   */
  private notifyMessage(message: any): void {
    this.messageHandlers.forEach((handler) => {
      try {
        handler(message);
      } catch (error) {
        this.notifyError(error as Error);
      }
    });
  }

  /**
   * Notify error handlers
   */
  private notifyError(error: Error): void {
    this.errorHandlers.forEach((handler) => {
      try {
        handler(error);
      } catch (err) {
        console.error('Error in error handler:', err);
      }
    });
  }

  /**
   * Notify connect handlers
   */
  private notifyConnect(): void {
    this.connectHandlers.forEach((handler) => {
      try {
        handler();
      } catch (error) {
        this.notifyError(error as Error);
      }
    });
  }

  /**
   * Notify disconnect handlers
   */
  private notifyDisconnect(): void {
    this.disconnectHandlers.forEach((handler) => {
      try {
        handler();
      } catch (error) {
        console.error('Error in disconnect handler:', error);
      }
    });
  }
}

