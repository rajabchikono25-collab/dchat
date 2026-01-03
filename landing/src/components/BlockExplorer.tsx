"use client";

import { useState, useEffect } from "react";
import { useNetworkData } from "@/lib/hooks";
import { Block, Transaction, createMockBlock } from "@/lib/api";
import { mockTransactions } from "@/data/mockBlocks";

function formatTime(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ago`;
}

function BlockRow({ block, isNew }: { block: Block; isNew: boolean }) {
  return (
    <div
      className={`grid grid-cols-4 md:grid-cols-6 gap-4 p-4 border-b border-white/5 transition-all duration-500 ${
        isNew ? "bg-purple-500/10 animate-pulse" : "hover:bg-white/5"
      }`}
    >
      <div className="flex items-center gap-2">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-500/20 to-blue-500/20 flex items-center justify-center">
          <svg
            className="w-4 h-4 text-purple-400"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4"
            />
          </svg>
        </div>
        <span className="font-mono text-purple-400">#{block.height}</span>
      </div>
      <div className="font-mono text-gray-400 text-sm truncate hidden md:block">
        {block.hash}
      </div>
      <div className="text-gray-400 text-sm">{formatTime(block.timestamp)}</div>
      <div className="text-white">{block.txCount} txns</div>
      <div className="text-gray-400 text-sm truncate hidden md:block">
        {block.validator}
      </div>
      <div className="text-gray-500 text-sm hidden md:block">
        {(block.size / 1024).toFixed(1)} KB
      </div>
    </div>
  );
}

function TransactionRow({ tx }: { tx: Transaction }) {
  const typeColors = {
    message: "bg-blue-500/20 text-blue-400",
    transfer: "bg-green-500/20 text-green-400",
    contract: "bg-purple-500/20 text-purple-400",
    stake: "bg-yellow-500/20 text-yellow-400",
  };

  return (
    <div className="grid grid-cols-3 md:grid-cols-5 gap-4 p-4 border-b border-white/5 hover:bg-white/5 transition-colors">
      <div className="font-mono text-blue-400 text-sm truncate">{tx.hash}</div>
      <div
        className={`px-2 py-1 rounded-full text-xs font-medium w-fit ${typeColors[tx.type]}`}
      >
        {tx.type}
      </div>
      <div className="text-gray-400 text-sm hidden md:block truncate">
        {tx.from}
      </div>
      <div className="text-gray-400 text-sm hidden md:block truncate">
        {tx.to}
      </div>
      <div className="text-white text-sm">{tx.value}</div>
    </div>
  );
}

function ConnectionBadge({ isLive }: { isLive: boolean }) {
  return (
    <div
      className={`flex items-center gap-2 px-3 py-1 rounded-full text-xs font-medium ${
        isLive
          ? "bg-green-500/20 text-green-400"
          : "bg-yellow-500/20 text-yellow-400"
      }`}
    >
      <div
        className={`w-2 h-2 rounded-full ${
          isLive ? "bg-green-400 animate-pulse" : "bg-yellow-400"
        }`}
      />
      {isLive ? "Live" : "Demo Mode"}
    </div>
  );
}

export default function BlockExplorer() {
  const { blocks, transactions, stats, isLive, isLoading, error } = useNetworkData();
  const [newBlockIndex, setNewBlockIndex] = useState(-1);
  const [activeTab, setActiveTab] = useState<"blocks" | "transactions">("blocks");
  const [localBlocks, setLocalBlocks] = useState<Block[]>([]);

  // Sync blocks from hook
  useEffect(() => {
    setLocalBlocks(blocks);
  }, [blocks]);

  // Simulate new blocks arriving when in demo mode
  useEffect(() => {
    if (isLive) return; // Don't simulate when connected to live API

    const interval = setInterval(() => {
      setLocalBlocks((prev) => {
        const height = prev[0]?.height ? prev[0].height + 1 : 2847564;
        const newBlock = createMockBlock(height);
        return [newBlock, ...prev.slice(0, 4)];
      });
      setNewBlockIndex(0);
      setTimeout(() => setNewBlockIndex(-1), 2000);
    }, 5000);

    return () => clearInterval(interval);
  }, [isLive]);

  if (isLoading) {
    return (
      <div className="glass rounded-3xl overflow-hidden">
        <div className="flex items-center justify-center h-96">
          <div className="flex items-center gap-3 text-gray-400">
            <svg
              className="w-6 h-6 animate-spin"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
              />
            </svg>
            <span>Connecting to network...</span>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="glass rounded-3xl overflow-hidden">
      {/* Stats bar */}
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4 p-6 border-b border-white/5 bg-gradient-to-r from-purple-500/5 to-blue-500/5">
        <div>
          <div className="text-2xl font-bold text-white">
            {stats.tps.toLocaleString()}
          </div>
          <div className="text-xs text-gray-500">TPS Capacity</div>
        </div>
        <div>
          <div className="text-2xl font-bold text-green-400">
            {stats.activeValidators}
          </div>
          <div className="text-xs text-gray-500">Active Validators</div>
        </div>
        <div>
          <div className="text-2xl font-bold text-purple-400">
            {stats.blockTime}
          </div>
          <div className="text-xs text-gray-500">Block Time</div>
        </div>
        <div className="hidden md:block">
          <div className="text-2xl font-bold text-blue-400">
            {(stats.totalBlocks / 1000000).toFixed(2)}M
          </div>
          <div className="text-xs text-gray-500">Total Blocks</div>
        </div>
        <div className="hidden lg:block">
          <div className="text-2xl font-bold text-white">
            {(stats.totalTransactions / 1000000).toFixed(0)}M
          </div>
          <div className="text-xs text-gray-500">Total Txns</div>
        </div>
        <div className="hidden lg:block">
          <div className="text-lg font-bold text-yellow-400 truncate">
            {stats.totalStaked}
          </div>
          <div className="text-xs text-gray-500">Total Staked</div>
        </div>
      </div>

      {/* Tabs with connection status */}
      <div className="flex items-center justify-between border-b border-white/5">
        <div className="flex">
          <button
            onClick={() => setActiveTab("blocks")}
            className={`px-6 py-3 font-medium transition-colors ${
              activeTab === "blocks"
                ? "text-purple-400 border-b-2 border-purple-400"
                : "text-gray-400 hover:text-white"
            }`}
          >
            Latest Blocks
          </button>
          <button
            onClick={() => setActiveTab("transactions")}
            className={`px-6 py-3 font-medium transition-colors ${
              activeTab === "transactions"
                ? "text-purple-400 border-b-2 border-purple-400"
                : "text-gray-400 hover:text-white"
            }`}
          >
            Recent Transactions
          </button>
        </div>
        <div className="pr-4">
          <ConnectionBadge isLive={isLive} />
        </div>
      </div>

      {/* Content */}
      <div className="max-h-[400px] overflow-y-auto">
        {activeTab === "blocks" ? (
          <>
            <div className="grid grid-cols-4 md:grid-cols-6 gap-4 px-4 py-2 text-xs text-gray-500 border-b border-white/5 sticky top-0 bg-[#0a0a0f]/90 backdrop-blur">
              <div>Block</div>
              <div className="hidden md:block">Hash</div>
              <div>Age</div>
              <div>Txns</div>
              <div className="hidden md:block">Validator</div>
              <div className="hidden md:block">Size</div>
            </div>
            {localBlocks.map((block, index) => (
              <BlockRow
                key={block.height}
                block={block}
                isNew={index === newBlockIndex}
              />
            ))}
          </>
        ) : (
          <>
            <div className="grid grid-cols-3 md:grid-cols-5 gap-4 px-4 py-2 text-xs text-gray-500 border-b border-white/5 sticky top-0 bg-[#0a0a0f]/90 backdrop-blur">
              <div>Tx Hash</div>
              <div>Type</div>
              <div className="hidden md:block">From</div>
              <div className="hidden md:block">To</div>
              <div>Value</div>
            </div>
            {transactions.length > 0
              ? transactions.map((tx) => <TransactionRow key={tx.hash} tx={tx} />)
              : mockTransactions.map((tx) => <TransactionRow key={tx.hash} tx={tx} />)}
          </>
        )}
      </div>

      {/* Status footer */}
      <div className="p-4 text-center text-xs text-gray-500 border-t border-white/5">
        {isLive ? (
          <span className="flex items-center justify-center gap-2">
            <span className="w-2 h-2 rounded-full bg-green-400 animate-pulse" />
            Connected to testnet - Real-time data
          </span>
        ) : (
          <span>
            {error
              ? `⚠️ ${error} - Using demo data`
              : "🔄 Demo mode - Configure NEXT_PUBLIC_API_URL for live data"}
          </span>
        )}
      </div>
    </div>
  );
}
