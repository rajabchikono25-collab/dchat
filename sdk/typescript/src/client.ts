/**
 * dchat Client implementation
 */

import { v4 as uuidv4 } from 'uuid';
import { ClientConfig, defaultClientConfig } from './config';
import { SdkError } from './errors';
import { Identity, Message, MessageStatus } from './types';
import { generateKeyPair, sign, verify } from './crypto/keypair';
import { WebSocketManager } from './crypto/websocket';

export class Client {
  private config: ClientConfig;
  private identity: Identity;
  private connected: boolean = false;
  private messages: Message[] = [];
  private wsManager: WebSocketManager | null = null;
  private keyPair: { publicKey: string; privateKey: string } | null = null;

  private constructor(
    config: ClientConfig,
    identity: Identity,
    keyPair: { publicKey: string; privateKey: string }
  ) {
    this.config = config;
    this.identity = identity;
    this.keyPair = keyPair;
  }

  /**
   * Create a new client builder
   */
  static builder(): ClientBuilder {
    return new ClientBuilder();
  }

  /**
   * Create a client with custom configuration
   */
  static async create(config: ClientConfig): Promise<Client> {
    // Generate Ed25519 key pair using @noble/ed25519
    const keyPair = await generateKeyPair();

    // Generate identity
    const identity: Identity = {
      userId: uuidv4(),
      username: config.name,
      publicKey: keyPair.publicKey,
      reputation: 0,
      createdAt: new Date(),
      verified: false,
      badges: [],
    };

    return new Client(config, identity, keyPair);
  }

  /**
   * Connect to the dchat network
   */
  async connect(): Promise<void> {
    if (this.connected) {
      throw SdkError.alreadyConnected();
    }

    // Select a relay from bootstrap peers
    const relayUrl = this.selectRelay();
    if (!relayUrl) {
      throw SdkError.config('No relay peers configured');
    }

    // Create WebSocket manager
    this.wsManager = new WebSocketManager({
      url: relayUrl,
      reconnectDelay: 5000,
      maxReconnectAttempts: 10,
      pingInterval: 30000,
    });

    // Set up message handler
    this.wsManager.onMessage((message) => {
      this.handleIncomingMessage(message);
    });

    // Set up error handler
    this.wsManager.onError((error) => {
      console.error('WebSocket error:', error);
    });

    // Connect to relay
    await this.wsManager.connect();
    this.connected = true;
  }

  /**
   * Select a relay from configured peers
   */
  private selectRelay(): string | null {
    const peers = this.config.network.bootstrapPeers;
    if (peers.length === 0) {
      return null;
    }

    // Simple round-robin selection - in production would use reputation scores
    const randomIndex = Math.floor(Math.random() * peers.length);
    const peer = peers[randomIndex];

    // Convert peer address to WebSocket URL
    // Assume peer format is "ip:port" or "ws://ip:port"
    if (peer.startsWith('ws://') || peer.startsWith('wss://')) {
      return peer;
    } else {
      return `ws://${peer}`;
    }
  }

  /**
   * Handle incoming message from relay
   */
  private handleIncomingMessage(data: any): void {
    try {
      // Parse and verify message
      if (data.type === 'message') {
        const message: Message = {
          id: data.id || uuidv4(),
          senderId: data.senderId,
          content: data.content,
          encryptedPayload: new Uint8Array(data.encryptedPayload || []),
          timestamp: new Date(data.timestamp),
          status: MessageStatus.Delivered,
          size: data.size || 0,
        };

        this.messages.push(message);
      }
    } catch (error) {
      console.error('Failed to handle incoming message:', error);
    }
  }

  /**
   * Disconnect from the network
   */
  async disconnect(): Promise<void> {
    if (!this.connected) {
      return;
    }

    if (this.wsManager) {
      await this.wsManager.disconnect();
      this.wsManager = null;
    }

    this.connected = false;
  }

  /**
   * Check if connected
   */
  isConnected(): boolean {
    return this.connected;
  }

  /**
   * Send a text message
   */
  async sendMessage(text: string): Promise<void> {
    if (!this.connected || !this.wsManager) {
      throw SdkError.notConnected();
    }

    if (!this.keyPair) {
      throw SdkError.config('Key pair not initialized');
    }

    const messageId = uuidv4();
    const timestamp = new Date();

    // Create message object
    const message: Message = {
      id: messageId,
      senderId: this.identity.userId,
      content: { type: 'Text', text },
      encryptedPayload: new Uint8Array(0), // TODO: Encrypt with recipient's public key
      timestamp,
      status: MessageStatus.Created,
      size: text.length,
    };

    // Sign the message
    const messageData = JSON.stringify({
      id: messageId,
      senderId: this.identity.userId,
      content: text,
      timestamp: timestamp.toISOString(),
    });
    const signature = await sign(messageData, this.keyPair.privateKey);

    // Send to relay via WebSocket
    await this.wsManager.send({
      type: 'send_message',
      message: {
        id: messageId,
        senderId: this.identity.userId,
        content: { type: 'Text', text },
        timestamp: timestamp.toISOString(),
        signature,
        publicKey: this.keyPair.publicKey,
      },
    });

    // Update local status
    message.status = MessageStatus.Sent;
    this.messages.push(message);
  }

  /**
   * Receive messages
   */
  async receiveMessages(): Promise<Message[]> {
    if (!this.connected) {
      throw SdkError.notConnected();
    }

    // Return messages received via WebSocket
    return [...this.messages];
  }

  /**
   * Get the client's identity
   */
  getIdentity(): Identity {
    return { ...this.identity };
  }

  /**
   * Get the client's configuration
   */
  getConfig(): ClientConfig {
    return { ...this.config };
  }

  /**
   * Sign a message with the client's private key
   */
  async signMessage(message: string): Promise<string> {
    if (!this.keyPair) {
      throw SdkError.config('Key pair not initialized');
    }

    return sign(message, this.keyPair.privateKey);
  }

  /**
   * Verify a message signature
   */
  async verifySignature(
    message: string,
    signature: string,
    publicKey: string
  ): Promise<boolean> {
    return verify(message, signature, publicKey);
  }
}

/**
 * Builder for creating a Client
 */
export class ClientBuilder {
  private config: ClientConfig;

  constructor() {
    this.config = defaultClientConfig();
  }

  /**
   * Set the user's display name
   */
  name(name: string): this {
    this.config.name = name;
    return this;
  }

  /**
   * Set the storage directory
   */
  dataDir(path: string): this {
    this.config.storage.dataDir = path;
    return this;
  }

  /**
   * Set bootstrap peers
   */
  bootstrapPeers(peers: string[]): this {
    this.config.network.bootstrapPeers = peers;
    return this;
  }

  /**
   * Set the listen port
   */
  listenPort(port: number): this {
    this.config.network.listenPort = port;
    return this;
  }

  /**
   * Enable or disable encryption
   */
  encryption(enabled: boolean): this {
    this.config.encryptionEnabled = enabled;
    return this;
  }

  /**
   * Build the client
   */
  async build(): Promise<Client> {
    return Client.create(this.config);
  }
}
