"use client";

import { useState } from "react";
import CodeBlock from "@/components/CodeBlock";

const codeExamples = {
  sendMessage: `import { DChat, Keypair } from '@dchat/sdk';

// Initialize client
const dchat = new DChat({
  network: 'mainnet',
  rpcUrl: 'https://rpc.dchat.network'
});

// Load your keypair
const keypair = Keypair.fromSecretKey(privateKey);

// Send an encrypted message
const result = await dchat.sendMessage({
  recipient: '0x742d35Cc6634C0532925a3b844Bc9e7595f8b3E1',
  content: 'Hello, decentralized world!',
  keypair
});

console.log('Message sent:', result.txHash);`,

  createChannel: `import { DChat, Channel } from '@dchat/sdk';

const dchat = new DChat({ network: 'mainnet' });

// Create a public channel
const channel = await dchat.createChannel({
  name: 'my-awesome-channel',
  description: 'A place for awesome discussions',
  type: 'public',
  keypair
});

// Subscribe to messages
channel.onMessage((msg) => {
  console.log(\`[\${msg.sender}]: \${msg.content}\`);
});

// Post to channel
await channel.post({
  content: 'Welcome to the channel!',
  keypair
});`,

  smartContract: `// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "@dchat/contracts/MessageRegistry.sol";

contract PremiumChannel is MessageRegistry {
    uint256 public subscriptionFee = 10 * 10**18; // 10 DCHAT
    
    mapping(address => bool) public subscribers;
    
    function subscribe() external payable {
        require(msg.value >= subscriptionFee, "Insufficient fee");
        subscribers[msg.sender] = true;
        emit Subscribed(msg.sender);
    }
    
    function postMessage(bytes calldata encryptedContent) 
        external 
        override 
        onlySubscribers 
    {
        _storeMessage(msg.sender, encryptedContent);
    }
    
    modifier onlySubscribers() {
        require(subscribers[msg.sender], "Not subscribed");
        _;
    }
}`,

  validateMessage: `import { DChat, MessageValidator } from '@dchat/sdk';

const dchat = new DChat({ network: 'mainnet' });

// Validate a message signature
const isValid = await dchat.validateMessage({
  messageHash: '0x8a7f2c...e3d1b9',
  signature: signatureBytes,
  sender: '0x742d35...8b9c1f'
});

// Verify ZK proof for anonymous messages
const zkProof = await dchat.verifyAnonymousMessage({
  proof: proofBytes,
  publicInputs: publicInputArray
});

console.log('Signature valid:', isValid);
console.log('ZK proof valid:', zkProof.valid);`,
};

const quickLinks = [
  { title: "SDK Reference", href: "/docs/sdk", icon: "📚" },
  { title: "API Endpoints", href: "/docs/api", icon: "🔌" },
  { title: "Smart Contracts", href: "/docs/contracts", icon: "📜" },
  { title: "CLI Tools", href: "/docs/cli", icon: "⌨️" },
  { title: "WebSocket Events", href: "/docs/websocket", icon: "🔄" },
  { title: "Security Guide", href: "/docs/security", icon: "🔐" },
];

export default function DevelopersContent() {
  const [activeExample, setActiveExample] =
    useState<keyof typeof codeExamples>("sendMessage");

  return (
    <div className="container mx-auto px-4 lg:px-8">
      {/* Hero Section */}
      <div className="text-center mb-16">
        <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass mb-6">
          <span className="text-sm text-blue-400 font-medium">
            For Developers
          </span>
        </div>
        <h1 className="text-4xl md:text-5xl lg:text-6xl font-bold mb-6">
          <span className="text-white">Build on </span>
          <span className="gradient-text">DChat</span>
        </h1>
        <p className="text-gray-400 max-w-3xl mx-auto text-lg">
          Everything you need to integrate decentralized messaging into your
          applications. TypeScript SDK, smart contracts, and comprehensive
          documentation.
        </p>
      </div>

      {/* Quick Start */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Quick Start
        </h2>
        <div className="max-w-3xl mx-auto">
          <CodeBlock
            title="terminal"
            language="bash"
            code={`# Install the DChat SDK
npm install @dchat/sdk

# Or with yarn
yarn add @dchat/sdk

# Generate a new keypair
npx dchat-cli keygen --output ./my-keypair.json`}
          />
        </div>
      </div>

      {/* Code Examples */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Code Examples
        </h2>
        <div className="max-w-5xl mx-auto">
          {/* Tabs */}
          <div className="flex flex-wrap gap-2 mb-6">
            {[
              { key: "sendMessage", label: "Send Message" },
              { key: "createChannel", label: "Create Channel" },
              { key: "smartContract", label: "Smart Contract" },
              { key: "validateMessage", label: "Validate Message" },
            ].map((tab) => (
              <button
                key={tab.key}
                onClick={() =>
                  setActiveExample(tab.key as keyof typeof codeExamples)
                }
                className={`px-4 py-2 rounded-lg text-sm font-medium transition-all ${
                  activeExample === tab.key
                    ? "bg-purple-500/20 text-purple-400 border border-purple-500/30"
                    : "text-gray-400 hover:text-white hover:bg-white/5"
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          {/* Code Block */}
          <CodeBlock
            title={`${activeExample}.${activeExample === "smartContract" ? "sol" : "ts"}`}
            language={
              activeExample === "smartContract" ? "solidity" : "typescript"
            }
            code={codeExamples[activeExample]}
          />
        </div>
      </div>

      {/* Quick Links */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Documentation
        </h2>
        <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-4 max-w-4xl mx-auto">
          {quickLinks.map((link) => (
            <a
              key={link.title}
              href={link.href}
              className="glass rounded-xl p-6 flex items-center gap-4 group hover:border-purple-500/30 transition-all"
            >
              <div className="text-3xl">{link.icon}</div>
              <div>
                <h3 className="text-white font-medium group-hover:text-purple-400 transition-colors">
                  {link.title}
                </h3>
                <p className="text-sm text-gray-500">View documentation →</p>
              </div>
            </a>
          ))}
        </div>
      </div>

      {/* Architecture Overview */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Architecture Overview
        </h2>
        <div className="grid lg:grid-cols-2 gap-8">
          {/* Message Flow */}
          <div className="glass rounded-2xl p-8">
            <h3 className="text-xl font-bold text-white mb-6">Message Flow</h3>
            <div className="space-y-4">
              <div className="flex items-start gap-4">
                <div className="w-8 h-8 rounded-lg bg-blue-500/20 flex items-center justify-center flex-shrink-0">
                  <span className="text-blue-400 font-bold">1</span>
                </div>
                <div>
                  <h4 className="font-medium text-white">Client Encryption</h4>
                  <p className="text-sm text-gray-400">
                    Message encrypted with recipient&apos;s public key using X25519 +
                    AES-256-GCM
                  </p>
                </div>
              </div>
              <div className="flex items-start gap-4">
                <div className="w-8 h-8 rounded-lg bg-blue-500/20 flex items-center justify-center flex-shrink-0">
                  <span className="text-blue-400 font-bold">2</span>
                </div>
                <div>
                  <h4 className="font-medium text-white">
                    Transaction Broadcast
                  </h4>
                  <p className="text-sm text-gray-400">
                    Encrypted blob submitted to mempool with sender signature
                  </p>
                </div>
              </div>
              <div className="flex items-start gap-4">
                <div className="w-8 h-8 rounded-lg bg-blue-500/20 flex items-center justify-center flex-shrink-0">
                  <span className="text-blue-400 font-bold">3</span>
                </div>
                <div>
                  <h4 className="font-medium text-white">
                    Validator Consensus
                  </h4>
                  <p className="text-sm text-gray-400">
                    Message included in block, finalized in 3 seconds
                  </p>
                </div>
              </div>
              <div className="flex items-start gap-4">
                <div className="w-8 h-8 rounded-lg bg-blue-500/20 flex items-center justify-center flex-shrink-0">
                  <span className="text-blue-400 font-bold">4</span>
                </div>
                <div>
                  <h4 className="font-medium text-white">Recipient Delivery</h4>
                  <p className="text-sm text-gray-400">
                    Client pulls from chain, decrypts with private key
                  </p>
                </div>
              </div>
            </div>
          </div>

          {/* Network Layers */}
          <div className="glass rounded-2xl p-8">
            <h3 className="text-xl font-bold text-white mb-6">
              Network Layers
            </h3>
            <div className="space-y-4">
              <div className="bg-white/5 rounded-xl p-4 border-l-4 border-purple-500">
                <h4 className="font-medium text-white">Application Layer</h4>
                <p className="text-sm text-gray-400">
                  SDK, CLI, Web3 providers, dApps
                </p>
              </div>
              <div className="bg-white/5 rounded-xl p-4 border-l-4 border-blue-500">
                <h4 className="font-medium text-white">Protocol Layer</h4>
                <p className="text-sm text-gray-400">
                  Message routing, channel management, identity
                </p>
              </div>
              <div className="bg-white/5 rounded-xl p-4 border-l-4 border-green-500">
                <h4 className="font-medium text-white">Consensus Layer</h4>
                <p className="text-sm text-gray-400">
                  Tendermint BFT, validator set, finality
                </p>
              </div>
              <div className="bg-white/5 rounded-xl p-4 border-l-4 border-yellow-500">
                <h4 className="font-medium text-white">Network Layer</h4>
                <p className="text-sm text-gray-400">
                  P2P gossip, block propagation, light clients
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* API Endpoints Preview */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          REST API Endpoints
        </h2>
        <div className="max-w-4xl mx-auto glass rounded-2xl overflow-hidden">
          <table className="w-full">
            <thead>
              <tr className="border-b border-white/10">
                <th className="text-left p-4 text-gray-400 font-medium">
                  Method
                </th>
                <th className="text-left p-4 text-gray-400 font-medium">
                  Endpoint
                </th>
                <th className="text-left p-4 text-gray-400 font-medium hidden md:table-cell">
                  Description
                </th>
              </tr>
            </thead>
            <tbody className="font-mono text-sm">
              <tr className="border-b border-white/5 hover:bg-white/5">
                <td className="p-4">
                  <span className="px-2 py-1 rounded bg-green-500/20 text-green-400">
                    POST
                  </span>
                </td>
                <td className="p-4 text-blue-400">/v1/messages</td>
                <td className="p-4 text-gray-400 hidden md:table-cell">
                  Send encrypted message
                </td>
              </tr>
              <tr className="border-b border-white/5 hover:bg-white/5">
                <td className="p-4">
                  <span className="px-2 py-1 rounded bg-blue-500/20 text-blue-400">
                    GET
                  </span>
                </td>
                <td className="p-4 text-blue-400">/v1/messages/:address</td>
                <td className="p-4 text-gray-400 hidden md:table-cell">
                  Get messages for address
                </td>
              </tr>
              <tr className="border-b border-white/5 hover:bg-white/5">
                <td className="p-4">
                  <span className="px-2 py-1 rounded bg-green-500/20 text-green-400">
                    POST
                  </span>
                </td>
                <td className="p-4 text-blue-400">/v1/channels</td>
                <td className="p-4 text-gray-400 hidden md:table-cell">
                  Create new channel
                </td>
              </tr>
              <tr className="border-b border-white/5 hover:bg-white/5">
                <td className="p-4">
                  <span className="px-2 py-1 rounded bg-blue-500/20 text-blue-400">
                    GET
                  </span>
                </td>
                <td className="p-4 text-blue-400">/v1/channels/:id/messages</td>
                <td className="p-4 text-gray-400 hidden md:table-cell">
                  Get channel messages
                </td>
              </tr>
              <tr className="border-b border-white/5 hover:bg-white/5">
                <td className="p-4">
                  <span className="px-2 py-1 rounded bg-blue-500/20 text-blue-400">
                    GET
                  </span>
                </td>
                <td className="p-4 text-blue-400">/v1/blocks/:height</td>
                <td className="p-4 text-gray-400 hidden md:table-cell">
                  Get block by height
                </td>
              </tr>
              <tr className="hover:bg-white/5">
                <td className="p-4">
                  <span className="px-2 py-1 rounded bg-blue-500/20 text-blue-400">
                    GET
                  </span>
                </td>
                <td className="p-4 text-blue-400">/v1/validators</td>
                <td className="p-4 text-gray-400 hidden md:table-cell">
                  Get active validators
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      {/* CTAs */}
      <div className="text-center space-x-4">
        <a
          href="/docs"
          className="inline-flex items-center gap-2 btn-primary px-8 py-4 rounded-xl font-semibold text-white"
        >
          Read Full Docs
          <svg
            className="w-5 h-5"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M17 8l4 4m0 0l-4 4m4-4H3"
            />
          </svg>
        </a>
        <a
          href="https://github.com/dchat"
          className="inline-flex items-center gap-2 glass px-8 py-4 rounded-xl font-semibold text-white hover:bg-white/10 transition-all"
        >
          <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
            <path
              fillRule="evenodd"
              d="M12 2C6.477 2 2 6.484 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0112 6.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0022 12.017C22 6.484 17.522 2 12 2z"
              clipRule="evenodd"
            />
          </svg>
          GitHub
        </a>
      </div>
    </div>
  );
}
