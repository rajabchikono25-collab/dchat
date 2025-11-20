/**
 * dchat TypeScript/JavaScript SDK
 * 
 * High-level API for building decentralized chat applications with blockchain integration.
 * 
 * @example
 * ```typescript
 * import { BlockchainClient, UserManager } from '@dchat/sdk';
 * 
 * const blockchain = BlockchainClient.local();
 * const userManager = new UserManager(blockchain, 'http://localhost:8080');
 * 
 * const user = await userManager.createUser('alice');
 * console.log('User created:', user.userId);
 * console.log('On-chain confirmed:', user.onChainConfirmed);
 * ```
 */

export { Client, ClientBuilder } from './client';
export { RelayNode } from './relay';
export {
  ClientConfig,
  StorageConfig,
  NetworkConfig,
  RelayConfig,
  defaultClientConfig,
  defaultStorageConfig,
  defaultNetworkConfig,
  defaultRelayConfig,
} from './config';
export { SdkError, ErrorCode } from './errors';
export {
  Identity,
  Message,
  MessageContent,
  MessageStatus,
  RelayStats,
} from './types';

// Blockchain integration
export { BlockchainClient, hashContent } from './blockchain/client';
export type { BlockchainConfig } from './blockchain/client';
export {
  TransactionStatus,
  ChannelVisibility,
} from './blockchain/transaction';
export type {
  TransactionReceipt,
  RegisterUserTx,
  SendDirectMessageTx,
  CreateChannelTx,
  PostToChannelTx,
  Transaction,
} from './blockchain/transaction';

// User management
export { UserManager } from './user/manager';
export type {
  CreateUserResponse,
  UserProfile,
  DirectMessageResponse,
  CreateChannelResponse,
  ChannelMessage,
  DirectMessage,
} from './user/models';

// Cryptographic utilities
export { generateKeyPair, sign, verify } from './crypto/keypair';
export type { KeyPair } from './crypto/keypair';
export { WebSocketManager } from './crypto/websocket';
export type {
  WebSocketConfig,
  MessageHandler,
  ErrorHandler,
  ConnectionHandler,
} from './crypto/websocket';

/**
 * SDK version
 */
export const VERSION = '0.1.0';

/**
 * Initialize the SDK
 */
export async function init(): Promise<void> {
  // Set up any global initialization
  return Promise.resolve();
}
