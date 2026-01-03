/**
 * React hooks for DChat API integration
 * 
 * Provides hooks for fetching network data with automatic fallback to mock data
 * when the API is unavailable (useful for development and demo purposes).
 */

'use client';

import { useState, useEffect, useCallback, useRef } from 'react';
import {
  Block,
  Transaction,
  NetworkStats,
  getLatestBlocks,
  getRecentTransactions,
  getNetworkStats,
  getPrometheusMetrics,
  createMockBlock,
  createMockNetworkStats,
  BlockExplorerWebSocket,
} from './api';
import { mockBlocks, mockTransactions } from '@/data/mockBlocks';

interface UseNetworkDataResult {
  blocks: Block[];
  transactions: Transaction[];
  stats: NetworkStats;
  isLive: boolean;
  isLoading: boolean;
  error: string | null;
  refresh: () => void;
}

/**
 * Main hook for block explorer data
 * 
 * Attempts to fetch live data from the API, falls back to mock data if unavailable.
 * Supports WebSocket for real-time updates when available.
 */
export function useNetworkData(enableWebSocket: boolean = false): UseNetworkDataResult {
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [stats, setStats] = useState<NetworkStats>(createMockNetworkStats());
  const [isLive, setIsLive] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  
  const wsRef = useRef<BlockExplorerWebSocket | null>(null);
  const pollIntervalRef = useRef<NodeJS.Timeout | null>(null);

  // Fetch initial data
  const fetchData = useCallback(async () => {
    setIsLoading(true);
    setError(null);

    try {
      // Try to fetch live data
      const [blocksResult, txResult, statsResult] = await Promise.all([
        getLatestBlocks(5),
        getRecentTransactions(5),
        getNetworkStats(),
      ]);

      if (blocksResult.success && blocksResult.data) {
        setBlocks(blocksResult.data);
        setIsLive(true);
      } else {
        // Fall back to mock data
        setBlocks(mockBlocks);
        setIsLive(false);
      }

      if (txResult.success && txResult.data) {
        setTransactions(txResult.data);
      } else {
        setTransactions(mockTransactions);
      }

      if (statsResult.success && statsResult.data) {
        setStats(statsResult.data);
      } else {
        setStats(createMockNetworkStats());
      }
    } catch (err) {
      // API unavailable - use mock data
      console.log('API unavailable, using mock data');
      setBlocks(mockBlocks);
      setTransactions(mockTransactions);
      setStats(createMockNetworkStats());
      setIsLive(false);
      setError(err instanceof Error ? err.message : 'Failed to connect to API');
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Start mock data simulation
  const startMockSimulation = useCallback(() => {
    let height = mockBlocks[0]?.height || 2847563;
    
    pollIntervalRef.current = setInterval(() => {
      height++;
      const newBlock = createMockBlock(height);
      setBlocks(prev => [newBlock, ...prev.slice(0, 4)]);
    }, 5000);
  }, []);

  // Initialize data fetching
  useEffect(() => {
    fetchData();

    // Set up WebSocket if enabled and supported
    if (enableWebSocket) {
      wsRef.current = new BlockExplorerWebSocket({
        onBlock: (block) => {
          setBlocks(prev => [block, ...prev.slice(0, 4)]);
        },
        onTransaction: (tx) => {
          setTransactions(prev => [tx, ...prev.slice(0, 4)]);
        },
        onStats: (newStats) => {
          setStats(newStats);
        },
        onConnect: () => {
          setIsLive(true);
          setError(null);
        },
        onDisconnect: () => {
          setIsLive(false);
          // Start mock simulation as fallback
          startMockSimulation();
        },
        onError: (err) => {
          setError(err);
        },
      });

      wsRef.current.connect();
    } else {
      // Poll for updates if no WebSocket
      const pollInterval = setInterval(() => {
        if (isLive) {
          fetchData();
        }
      }, 10000);

      // If not live, simulate new blocks
      if (!isLive) {
        startMockSimulation();
      }

      return () => {
        clearInterval(pollInterval);
      };
    }

    return () => {
      wsRef.current?.disconnect();
      if (pollIntervalRef.current) {
        clearInterval(pollIntervalRef.current);
      }
    };
  }, [fetchData, enableWebSocket, isLive, startMockSimulation]);

  const refresh = useCallback(() => {
    fetchData();
  }, [fetchData]);

  return {
    blocks,
    transactions,
    stats,
    isLive,
    isLoading,
    error,
    refresh,
  };
}

/**
 * Hook for fetching Prometheus metrics
 */
export function usePrometheusMetrics(refreshInterval: number = 30000) {
  const [metrics, setMetrics] = useState<Record<string, number>>({});
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchMetrics = useCallback(async () => {
    const result = await getPrometheusMetrics();
    if (result.success && result.data) {
      setMetrics(result.data);
      setError(null);
    } else {
      setError(result.error || 'Failed to fetch metrics');
    }
    setIsLoading(false);
  }, []);

  useEffect(() => {
    fetchMetrics();
    const interval = setInterval(fetchMetrics, refreshInterval);
    return () => clearInterval(interval);
  }, [fetchMetrics, refreshInterval]);

  return { metrics, isLoading, error, refresh: fetchMetrics };
}

/**
 * Hook for tracking connection status
 */
export function useConnectionStatus() {
  const [isOnline, setIsOnline] = useState(true);
  const [apiStatus, setApiStatus] = useState<'connected' | 'disconnected' | 'checking'>('checking');

  useEffect(() => {
    // Browser online/offline detection
    const handleOnline = () => setIsOnline(true);
    const handleOffline = () => setIsOnline(false);

    window.addEventListener('online', handleOnline);
    window.addEventListener('offline', handleOffline);

    // Check API connectivity
    const checkApi = async () => {
      try {
        const response = await fetch(`${process.env.NEXT_PUBLIC_API_URL || 'http://localhost:9000'}/v1/health`, {
          method: 'GET',
          signal: AbortSignal.timeout(3000),
        });
        setApiStatus(response.ok ? 'connected' : 'disconnected');
      } catch {
        setApiStatus('disconnected');
      }
    };

    checkApi();
    const interval = setInterval(checkApi, 30000);

    return () => {
      window.removeEventListener('online', handleOnline);
      window.removeEventListener('offline', handleOffline);
      clearInterval(interval);
    };
  }, []);

  return { isOnline, apiStatus };
}
