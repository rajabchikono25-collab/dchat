/**
 * Cryptographic utilities for key management
 * Uses @noble/ed25519 for real Ed25519 cryptography
 */

import * as ed25519 from '@noble/ed25519';

export interface KeyPair {
  publicKey: string;
  privateKey: string;
}

/**
 * Generate a new Ed25519 key pair
 * Uses @noble/ed25519 for secure key generation
 */
export async function generateKeyPair(): Promise<KeyPair> {
  const privateKey = ed25519.utils.randomPrivateKey();
  const publicKey = await ed25519.getPublicKeyAsync(privateKey);

  return {
    publicKey: Buffer.from(publicKey).toString('hex'),
    privateKey: Buffer.from(privateKey).toString('hex'),
  };
}

/**
 * Sign a message with a private key
 * Uses Ed25519 signature algorithm
 */
export async function sign(message: string, privateKey: string): Promise<string> {
  const messageBytes = Buffer.from(message, 'utf8');
  const privateKeyBytes = Buffer.from(privateKey, 'hex');
  
  const signature = await ed25519.signAsync(messageBytes, privateKeyBytes);
  return Buffer.from(signature).toString('hex');
}

/**
 * Verify a signature
 * Uses Ed25519 signature verification
 */
export async function verify(
  message: string,
  signature: string,
  publicKey: string
): Promise<boolean> {
  try {
    const messageBytes = Buffer.from(message, 'utf8');
    const signatureBytes = Buffer.from(signature, 'hex');
    const publicKeyBytes = Buffer.from(publicKey, 'hex');
    
    return await ed25519.verifyAsync(signatureBytes, messageBytes, publicKeyBytes);
  } catch (error) {
    return false;
  }
}
