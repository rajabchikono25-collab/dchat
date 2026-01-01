// Static mock data for block explorer - placeholder for future testnet WebSocket integration

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
  type: "message" | "transfer" | "contract" | "stake";
  timestamp: number;
  status: "confirmed" | "pending";
}

export const mockBlocks: Block[] = [
  {
    height: 2847563,
    hash: "0x8a7f2c...e3d1b9",
    timestamp: Date.now() - 3000,
    txCount: 142,
    validator: "validator-india-1",
    size: 48256,
    gasUsed: 12450000,
  },
  {
    height: 2847562,
    hash: "0x4b2e1a...f7c8d2",
    timestamp: Date.now() - 6000,
    txCount: 98,
    validator: "validator-uae-1",
    size: 32180,
    gasUsed: 9820000,
  },
  {
    height: 2847561,
    hash: "0xc9d3f7...a1e4b8",
    timestamp: Date.now() - 9000,
    txCount: 156,
    validator: "validator-southafrica-1",
    size: 52340,
    gasUsed: 14200000,
  },
  {
    height: 2847560,
    hash: "0x1f8e2b...d6c9a3",
    timestamp: Date.now() - 12000,
    txCount: 87,
    validator: "validator-india-2",
    size: 28760,
    gasUsed: 8650000,
  },
  {
    height: 2847559,
    hash: "0xe7a4c1...b2f5d8",
    timestamp: Date.now() - 15000,
    txCount: 203,
    validator: "validator-uae-2",
    size: 68420,
    gasUsed: 18900000,
  },
];

export const mockTransactions: Transaction[] = [
  {
    hash: "0x2f4a8c...e7b1d3",
    from: "0x742d35...8b9c1f",
    to: "0xa93f21...c4e8b2",
    value: "0 DCHAT",
    type: "message",
    timestamp: Date.now() - 1500,
    status: "confirmed",
  },
  {
    hash: "0x9e1b7f...a3c2d4",
    from: "0x8f2e1a...d7b3c9",
    to: "0xb47c32...f1e9a8",
    value: "150 DCHAT",
    type: "transfer",
    timestamp: Date.now() - 2200,
    status: "confirmed",
  },
  {
    hash: "0x3c5d9a...f8e2b1",
    from: "0x521b8e...a9c4f2",
    to: "0x0000000000000000",
    value: "0 DCHAT",
    type: "contract",
    timestamp: Date.now() - 3100,
    status: "confirmed",
  },
  {
    hash: "0x7a2e4b...c1d8f9",
    from: "0xd93a1f...e7b2c4",
    to: "staking-contract",
    value: "5000 DCHAT",
    type: "stake",
    timestamp: Date.now() - 4500,
    status: "confirmed",
  },
  {
    hash: "0x1e9c3f...d4a7b2",
    from: "0x6b8f2e...c1a9d4",
    to: "0x3f7e9a...b2c8d1",
    value: "0 DCHAT",
    type: "message",
    timestamp: Date.now() - 5800,
    status: "pending",
  },
];

export const networkStats = {
  tps: 10000,
  activeValidators: 21,
  totalStaked: "847,500,000 DCHAT",
  blockTime: "3s",
  totalBlocks: 2847563,
  totalTransactions: 142847520,
};
