"use client";

import { useState } from "react";
import ExpandableCard from "@/components/ExpandableCard";
import BlockExplorer from "@/components/BlockExplorer";

export default function BlockchainContent() {
  const [expandedCard, setExpandedCard] = useState<string | null>(null);
  const [isMobile, setIsMobile] = useState(false);

  // Check for mobile on mount
  if (typeof window !== "undefined" && !isMobile) {
    if (window.innerWidth < 768) {
      setIsMobile(true);
    }
  }

  const handleToggle = (cardId: string) => {
    if (isMobile) {
      setExpandedCard(expandedCard === cardId ? null : cardId);
    }
  };

  return (
    <div className="container mx-auto px-4 lg:px-8">
      {/* Hero Section */}
      <div className="text-center mb-16">
        <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass mb-6">
          <span className="text-sm text-purple-400 font-medium">
            Architecture
          </span>
        </div>
        <h1 className="text-4xl md:text-5xl lg:text-6xl font-bold mb-6">
          <span className="text-white">Tri-Chain </span>
          <span className="gradient-text-purple">Architecture</span>
        </h1>
        <p className="text-gray-400 max-w-3xl mx-auto text-lg">
          DChat leverages a unique three-chain architecture to achieve
          unprecedented scalability, security, and decentralization for
          messaging at global scale.
        </p>
      </div>

      {/* Chain Architecture Visual */}
      <div className="mb-20">
        <div className="grid md:grid-cols-3 gap-6 mb-8">
          {/* Message Chain */}
          <div className="glass rounded-3xl p-8 relative overflow-hidden group">
            <div className="absolute inset-0 bg-gradient-to-br from-blue-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity" />
            <div className="relative">
              <div className="w-16 h-16 rounded-2xl bg-gradient-to-br from-blue-500 to-cyan-500 flex items-center justify-center mb-6">
                <svg
                  className="w-8 h-8 text-white"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"
                  />
                </svg>
              </div>
              <h3 className="text-2xl font-bold text-white mb-3">
                Message Chain
              </h3>
              <p className="text-gray-400 mb-4">
                High-throughput chain optimized for encrypted message delivery
                with 3-second finality.
              </p>
              <div className="space-y-2">
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-blue-500 rounded-full" />
                  <span className="text-gray-300">10,000+ TPS capacity</span>
                </div>
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-blue-500 rounded-full" />
                  <span className="text-gray-300">Ephemeral block pruning</span>
                </div>
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-blue-500 rounded-full" />
                  <span className="text-gray-300">Zero-knowledge proofs</span>
                </div>
              </div>
            </div>
          </div>

          {/* Currency Chain */}
          <div className="glass rounded-3xl p-8 relative overflow-hidden group">
            <div className="absolute inset-0 bg-gradient-to-br from-green-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity" />
            <div className="relative">
              <div className="w-16 h-16 rounded-2xl bg-gradient-to-br from-green-500 to-emerald-500 flex items-center justify-center mb-6">
                <svg
                  className="w-8 h-8 text-white"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M12 8c-1.657 0-3 .895-3 2s1.343 2 3 2 3 .895 3 2-1.343 2-3 2m0-8c1.11 0 2.08.402 2.599 1M12 8V7m0 1v8m0 0v1m0-1c-1.11 0-2.08-.402-2.599-1M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                  />
                </svg>
              </div>
              <h3 className="text-2xl font-bold text-white mb-3">
                Currency Chain
              </h3>
              <p className="text-gray-400 mb-4">
                Secure value transfer and DeFi operations with DCHAT token
                economics.
              </p>
              <div className="space-y-2">
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-green-500 rounded-full" />
                  <span className="text-gray-300">EVM-compatible</span>
                </div>
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-green-500 rounded-full" />
                  <span className="text-gray-300">Smart contract support</span>
                </div>
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-green-500 rounded-full" />
                  <span className="text-gray-300">Cross-chain bridges</span>
                </div>
              </div>
            </div>
          </div>

          {/* Governance Chain */}
          <div className="glass rounded-3xl p-8 relative overflow-hidden group">
            <div className="absolute inset-0 bg-gradient-to-br from-purple-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity" />
            <div className="relative">
              <div className="w-16 h-16 rounded-2xl bg-gradient-to-br from-purple-500 to-pink-500 flex items-center justify-center mb-6">
                <svg
                  className="w-8 h-8 text-white"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"
                  />
                </svg>
              </div>
              <h3 className="text-2xl font-bold text-white mb-3">
                Governance Chain
              </h3>
              <p className="text-gray-400 mb-4">
                Decentralized protocol upgrades and validator coordination.
              </p>
              <div className="space-y-2">
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-purple-500 rounded-full" />
                  <span className="text-gray-300">On-chain voting</span>
                </div>
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-purple-500 rounded-full" />
                  <span className="text-gray-300">Proposal system</span>
                </div>
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 bg-purple-500 rounded-full" />
                  <span className="text-gray-300">Validator elections</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Block Explorer */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Live Block Explorer
        </h2>
        <BlockExplorer />
      </div>

      {/* Technical Deep Dives */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Technical Deep Dive
        </h2>
        <div className="space-y-4 max-w-4xl mx-auto">
          <ExpandableCard
            title="Block Hierarchy & Structure"
            icon={
              <svg
                className="w-6 h-6 text-purple-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"
                />
              </svg>
            }
            summary="Three-tier block structure with epochs, shards, and micro-blocks for optimal throughput."
            accentColor="purple"
            singleAccordion={isMobile}
            isExpanded={expandedCard === "block-hierarchy"}
            onToggle={() => handleToggle("block-hierarchy")}
          >
            <div className="space-y-4 text-gray-300">
              <p>
                DChat uses a hierarchical block structure designed for
                message-centric workloads:
              </p>
              <div className="grid md:grid-cols-3 gap-4 my-6">
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    Epoch Blocks
                  </h4>
                  <p className="text-sm text-gray-400">
                    Finalized every 100 blocks (~5 minutes). Contains validator
                    set changes and cross-chain anchors.
                  </p>
                </div>
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    Shard Blocks
                  </h4>
                  <p className="text-sm text-gray-400">
                    Parallel execution across 64 shards. Each shard processes
                    ~150 TPS independently.
                  </p>
                </div>
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    Micro-Blocks
                  </h4>
                  <p className="text-sm text-gray-400">
                    Sub-second message blocks for real-time delivery.
                    Optimistically confirmed, then batched.
                  </p>
                </div>
              </div>
              <p>
                This structure allows message delivery latency of &lt;500ms
                while maintaining strong finality guarantees at the epoch level.
              </p>
            </div>
          </ExpandableCard>

          <ExpandableCard
            title="Consensus Mechanism"
            icon={
              <svg
                className="w-6 h-6 text-blue-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"
                />
              </svg>
            }
            summary="Hybrid PoS with BFT finality - combining speed with security."
            accentColor="blue"
            singleAccordion={isMobile}
            isExpanded={expandedCard === "consensus"}
            onToggle={() => handleToggle("consensus")}
          >
            <div className="space-y-4 text-gray-300">
              <p>
                DChat implements a custom consensus called{" "}
                <strong className="text-white">
                  Proof-of-Stake with Tendermint BFT
                </strong>
                :
              </p>
              <ul className="list-disc list-inside space-y-2 ml-4">
                <li>
                  <strong className="text-white">21 active validators</strong> -
                  Elected through governance, rotated quarterly
                </li>
                <li>
                  <strong className="text-white">2/3 threshold</strong> -
                  Byzantine fault tolerance up to 7 malicious validators
                </li>
                <li>
                  <strong className="text-white">3-second blocks</strong> -
                  Optimized for message throughput
                </li>
                <li>
                  <strong className="text-white">Single-slot finality</strong> -
                  No reorganizations after block commit
                </li>
              </ul>
              <div className="bg-purple-500/10 rounded-xl p-4 mt-4">
                <h4 className="font-semibold text-purple-400 mb-2">
                  Slashing Conditions
                </h4>
                <p className="text-sm">
                  Validators are slashed for double-signing (100% stake),
                  prolonged downtime (5% stake), or censorship attempts (25%
                  stake).
                </p>
              </div>
            </div>
          </ExpandableCard>

          <ExpandableCard
            title="Privacy & Zero-Knowledge Proofs"
            icon={
              <svg
                className="w-6 h-6 text-green-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
                />
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z"
                />
              </svg>
            }
            summary="Message privacy with zkSNARKs and encrypted metadata."
            accentColor="green"
            singleAccordion={isMobile}
            isExpanded={expandedCard === "privacy"}
            onToggle={() => handleToggle("privacy")}
          >
            <div className="space-y-4 text-gray-300">
              <p>DChat employs multiple layers of privacy protection:</p>
              <div className="space-y-3">
                <div className="flex items-start gap-3">
                  <div className="w-8 h-8 rounded-lg bg-green-500/20 flex items-center justify-center flex-shrink-0 mt-1">
                    <span className="text-green-400 font-bold">1</span>
                  </div>
                  <div>
                    <h4 className="font-semibold text-white">
                      End-to-End Encryption
                    </h4>
                    <p className="text-sm text-gray-400">
                      All messages encrypted with X25519 key exchange and
                      AES-256-GCM. Keys never leave user devices.
                    </p>
                  </div>
                </div>
                <div className="flex items-start gap-3">
                  <div className="w-8 h-8 rounded-lg bg-green-500/20 flex items-center justify-center flex-shrink-0 mt-1">
                    <span className="text-green-400 font-bold">2</span>
                  </div>
                  <div>
                    <h4 className="font-semibold text-white">
                      Metadata Protection
                    </h4>
                    <p className="text-sm text-gray-400">
                      Sender/recipient information hidden using zkSNARK proofs.
                      Validators see only encrypted blobs.
                    </p>
                  </div>
                </div>
                <div className="flex items-start gap-3">
                  <div className="w-8 h-8 rounded-lg bg-green-500/20 flex items-center justify-center flex-shrink-0 mt-1">
                    <span className="text-green-400 font-bold">3</span>
                  </div>
                  <div>
                    <h4 className="font-semibold text-white">
                      Anonymous Channels
                    </h4>
                    <p className="text-sm text-gray-400">
                      Optional onion routing for high-privacy communications.
                      Adds ~200ms latency.
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </ExpandableCard>

          <ExpandableCard
            title="Network Hardening"
            icon={
              <svg
                className="w-6 h-6 text-pink-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"
                />
              </svg>
            }
            summary="DDoS protection, Sybil resistance, and attack mitigation strategies."
            accentColor="pink"
            singleAccordion={isMobile}
            isExpanded={expandedCard === "hardening"}
            onToggle={() => handleToggle("hardening")}
          >
            <div className="space-y-4 text-gray-300">
              <p>DChat is hardened against common attack vectors:</p>
              <div className="grid md:grid-cols-2 gap-4">
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    🛡️ Sybil Resistance
                  </h4>
                  <p className="text-sm text-gray-400">
                    Minimum 100,000 DCHAT stake required to run a validator.
                    Economic disincentives for malicious behavior.
                  </p>
                </div>
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    🌐 DDoS Protection
                  </h4>
                  <p className="text-sm text-gray-400">
                    Rate limiting at protocol level. Message fees prevent spam.
                    Validators distributed across 6 continents.
                  </p>
                </div>
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    🔐 Eclipse Prevention
                  </h4>
                  <p className="text-sm text-gray-400">
                    Gossip protocol with randomized peer selection. Minimum 50
                    peer connections per node.
                  </p>
                </div>
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    ⚡ Long-Range Attacks
                  </h4>
                  <p className="text-sm text-gray-400">
                    Weak subjectivity checkpoints every 1000 epochs. Social
                    consensus for chain selection.
                  </p>
                </div>
              </div>
            </div>
          </ExpandableCard>
        </div>
      </div>

      {/* CTA */}
      <div className="text-center">
        <a
          href="/developers"
          className="inline-flex items-center gap-2 btn-primary px-8 py-4 rounded-xl font-semibold text-white"
        >
          Start Building
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
      </div>
    </div>
  );
}
