/**
 * DChat API Client
 * 
 * Production API client for connecting to dchat node endpoints.
 * Supports both REST API and WebSocket for real-time updates.
 */

// API base URLs - configurable via environment variables
const API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:9000';
const WS_URL = process.env.NEXT_PUBLIC_WS_URL || 'ws://localhost:9001';
const METRICS_URL = process.env.NEXT_PUBLIC_METRICS_URL || 'http://localhost:9090';

// Types for API responses
export interface Block {
  height: number;
  hash: string;
  timestamp: number;
  txCount: number;
  validator: string;
  size: number;
  gasUsed: number;
}

export interface Transaction {
  hash: string;
  from: string;
  to: string;
  value: string;
  type: 'message' | 'transfer' | 'contract' | 'stake';
  timestamp: number;
  status: 'confirmed' | 'pending';
}

export interface NetworkStats {
  tps: number;
  activeValidators: number;
  totalStaked: string;
  blockTime: string;
  totalBlocks: number;
  totalTransactions: number;
}

export interface Validator {
  id: string;
  name: string;
  region: string;
  stake: number;
  uptime: number;
  blocksProduced: number;
  status: 'active' | 'inactive' | 'jailed';
}

export interface ApiResponse<T> {
  success: boolean;
  data?: T;
  error?: string;
}

/**
 * Fetch with timeout and error handling
 */
async function fetchWithTimeout<T>(
  url: string,
  options: RequestInit = {},
  timeoutMs: number = 5000
): Promise<ApiResponse<T>> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const response = await fetch(url, {
      ...options,
      signal: controller.signal,
      headers: {
        'Content-Type': 'application/json',
        ...options.headers,
      },
    });

    clearTimeout(timeout);

    if (!response.ok) {
      return {
        success: false,
        error: `HTTP ${response.status}: ${response.statusText}`,
      };
    }

    const data = await response.json();
    return { success: true, data };
  } catch (err) {
    clearTimeout(timeout);
    if (err instanceof Error) {
      if (err.name === 'AbortError') {
        return { success: false, error: 'Request timeout' };
      }
      return { success: false, error: err.message };
    }
    return { success: false, error: 'Unknown error' };
  }
}

/**
 * Get latest blocks from the chat chain
 */
export async function getLatestBlocks(limit: number = 10): Promise<ApiResponse<Block[]>> {
  return fetchWithTimeout<Block[]>(`${API_BASE_URL}/v1/blocks?limit=${limit}`);
}

/**
 * Get recent transactions
 */
export async function getRecentTransactions(limit: number = 10): Promise<ApiResponse<Transaction[]>> {
  return fetchWithTimeout<Transaction[]>(`${API_BASE_URL}/v1/transactions?limit=${limit}`);
}

/**
 * Get network statistics
 */
export async function getNetworkStats(): Promise<ApiResponse<NetworkStats>> {
  return fetchWithTimeout<NetworkStats>(`${API_BASE_URL}/v1/stats`);
}

/**
 * Get validator list
 */
export async function getValidators(): Promise<ApiResponse<Validator[]>> {
  return fetchWithTimeout<Validator[]>(`${API_BASE_URL}/v1/validators`);
}

/**
 * Get block by height
 */
export async function getBlockByHeight(height: number): Promise<ApiResponse<Block>> {
  return fetchWithTimeout<Block>(`${API_BASE_URL}/v1/blocks/${height}`);
}

/**
 * Get transaction by hash
 */
export async function getTransactionByHash(hash: string): Promise<ApiResponse<Transaction>> {
  return fetchWithTimeout<Transaction>(`${API_BASE_URL}/v1/transactions/${hash}`);
}

/**
 * Parse Prometheus metrics into structured data
 */
export function parsePrometheusMetrics(metricsText: string): Record<string, number> {
  const metrics: Record<string, number> = {};
  const lines = metricsText.split('\n');

  for (const line of lines) {
    // Skip comments and empty lines
    if (line.startsWith('#') || !line.trim()) continue;

    // Parse metric line: metric_name{labels} value
    const match = line.match(/^([a-zA-Z_:][a-zA-Z0-9_:]*)\s+([0-9.e+-]+)/);
    if (match) {
      metrics[match[1]] = parseFloat(match[2]);
    }
  }

  return metrics;
}

/**
 * Fetch raw Prometheus metrics
 */
export async function getPrometheusMetrics(): Promise<ApiResponse<Record<string, number>>> {
  try {
    const response = await fetch(`${METRICS_URL}/metrics`);
    if (!response.ok) {
      return { success: false, error: `HTTP ${response.status}` };
    }
    const text = await response.text();
    const metrics = parsePrometheusMetrics(text);
    return { success: true, data: metrics };
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : 'Unknown error' };
  }
}

/**
 * WebSocket connection manager for real-time updates
 */
export class BlockExplorerWebSocket {
  private ws: WebSocket | null = null;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;
  private reconnectDelay = 1000;
  private handlers: {
    onBlock?: (block: Block) => void;
    onTransaction?: (tx: Transaction) => void;
    onStats?: (stats: NetworkStats) => void;
    onError?: (error: string) => void;
    onConnect?: () => void;
    onDisconnect?: () => void;
  } = {};

  constructor(handlers: typeof this.handlers = {}) {
    this.handlers = handlers;
  }

  connect() {
    if (this.ws?.readyState === WebSocket.OPEN) {
      return;
    }

    try {
      this.ws = new WebSocket(`${WS_URL}/ws/explorer`);

      this.ws.onopen = () => {
        console.log('WebSocket connected');
        this.reconnectAttempts = 0;
        this.handlers.onConnect?.();
      };

      this.ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data);
          switch (message.type) {
            case 'new_block':
              this.handlers.onBlock?.(message.data);
              break;
            case 'new_transaction':
              this.handlers.onTransaction?.(message.data);
              break;
            case 'stats_update':
              this.handlers.onStats?.(message.data);
              break;
          }
        } catch {
          console.error('Failed to parse WebSocket message');
        }
      };

      this.ws.onerror = () => {
        this.handlers.onError?.('WebSocket error');
      };

      this.ws.onclose = () => {
        this.handlers.onDisconnect?.();
        this.attemptReconnect();
      };
    } catch (err) {
      this.handlers.onError?.(err instanceof Error ? err.message : 'Connection failed');
    }
  }

  private attemptReconnect() {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.log('Max reconnect attempts reached');
      return;
    }

    this.reconnectAttempts++;
    const delay = this.reconnectDelay * Math.pow(2, this.reconnectAttempts - 1);
    
    setTimeout(() => {
      console.log(`Reconnecting (attempt ${this.reconnectAttempts})...`);
      this.connect();
    }, delay);
  }

  disconnect() {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }
}

/**
 * Create mock data for demo mode when API is unavailable
 */
export function createMockNetworkStats(): NetworkStats {
  return {
    tps: 10000,
    activeValidators: 21,
    totalStaked: '847,500,000 DCHAT',
    blockTime: '3s',
    totalBlocks: 2847563,
    totalTransactions: 142847520,
  };
}

export function createMockBlock(height: number): Block {
  const validators = ['validator-india-1', 'validator-uae-1', 'validator-southafrica-1'];
  return {
    height,
    hash: `0x${Math.random().toString(16).slice(2, 10)}...${Math.random().toString(16).slice(2, 8)}`,
    timestamp: Date.now(),
    txCount: Math.floor(Math.random() * 200) + 50,
    validator: validators[Math.floor(Math.random() * validators.length)],
    size: Math.floor(Math.random() * 50000) + 20000,
    gasUsed: Math.floor(Math.random() * 15000000) + 5000000,
  };
}
