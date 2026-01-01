"use client";

import { useState } from "react";
import ExpandableCard from "@/components/ExpandableCard";

const tokenDistribution = [
  {
    label: "Community & Ecosystem",
    percentage: 40,
    color: "from-purple-500 to-blue-500",
  },
  {
    label: "Staking Rewards",
    percentage: 25,
    color: "from-green-500 to-emerald-500",
  },
  {
    label: "Team & Advisors",
    percentage: 15,
    color: "from-yellow-500 to-orange-500",
  },
  {
    label: "Foundation Reserve",
    percentage: 10,
    color: "from-pink-500 to-rose-500",
  },
  {
    label: "Liquidity & Partnerships",
    percentage: 10,
    color: "from-blue-500 to-cyan-500",
  },
];

const vestingSchedule = [
  {
    category: "Community",
    cliff: "None",
    vesting: "Linear over 4 years",
    tge: "10%",
  },
  {
    category: "Team",
    cliff: "12 months",
    vesting: "Linear over 3 years",
    tge: "0%",
  },
  {
    category: "Advisors",
    cliff: "6 months",
    vesting: "Linear over 2 years",
    tge: "0%",
  },
  {
    category: "Foundation",
    cliff: "6 months",
    vesting: "Linear over 5 years",
    tge: "5%",
  },
];

export default function TokenomicsContent() {
  const [expandedCard, setExpandedCard] = useState<string | null>(null);
  const [isMobile, setIsMobile] = useState(false);

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
          <span className="text-sm text-green-400 font-medium">
            Token Economics
          </span>
        </div>
        <h1 className="text-4xl md:text-5xl lg:text-6xl font-bold mb-6">
          <span className="text-white">DCHAT </span>
          <span className="gradient-text-green">Tokenomics</span>
        </h1>
        <p className="text-gray-400 max-w-3xl mx-auto text-lg">
          The DCHAT token powers the entire ecosystem - from messaging fees to
          governance votes. Designed for long-term sustainability and
          decentralization.
        </p>
      </div>

      {/* Token Overview */}
      <div className="grid md:grid-cols-4 gap-6 mb-16">
        <div className="glass rounded-2xl p-6 text-center">
          <div className="text-3xl font-bold gradient-text-green mb-2">1B</div>
          <div className="text-sm text-gray-400">Total Supply</div>
        </div>
        <div className="glass rounded-2xl p-6 text-center">
          <div className="text-3xl font-bold text-purple-400 mb-2">847.5M</div>
          <div className="text-sm text-gray-400">Circulating Supply</div>
        </div>
        <div className="glass rounded-2xl p-6 text-center">
          <div className="text-3xl font-bold text-blue-400 mb-2">2%</div>
          <div className="text-sm text-gray-400">Annual Inflation</div>
        </div>
        <div className="glass rounded-2xl p-6 text-center">
          <div className="text-3xl font-bold text-yellow-400 mb-2">50%</div>
          <div className="text-sm text-gray-400">Fee Burn Rate</div>
        </div>
      </div>

      {/* Token Distribution */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Token Distribution
        </h2>
        <div className="grid lg:grid-cols-2 gap-8 items-center">
          {/* Pie Chart Visual */}
          <div className="relative aspect-square max-w-md mx-auto">
            <div className="absolute inset-0 flex items-center justify-center">
              <div className="text-center">
                <div className="text-4xl font-bold text-white">1B</div>
                <div className="text-gray-400">DCHAT</div>
              </div>
            </div>
            <svg viewBox="0 0 100 100" className="w-full h-full -rotate-90">
              {
                tokenDistribution.reduce(
                  (acc, item, index) => {
                    const offset = acc.offset;
                    const circumference = 2 * Math.PI * 35;
                    const dashArray = (item.percentage / 100) * circumference;
                    const dashOffset = -offset;
                    acc.elements.push(
                      <circle
                        key={item.label}
                        cx="50"
                        cy="50"
                        r="35"
                        fill="none"
                        stroke={`url(#gradient-${index})`}
                        strokeWidth="20"
                        strokeDasharray={`${dashArray} ${circumference}`}
                        strokeDashoffset={dashOffset}
                        className="transition-all duration-500"
                      />,
                    );
                    acc.offset += dashArray;
                    return acc;
                  },
                  { elements: [] as React.ReactNode[], offset: 0 },
                ).elements
              }
              <defs>
                {tokenDistribution.map((item, index) => (
                  <linearGradient key={index} id={`gradient-${index}`}>
                    <stop
                      offset="0%"
                      stopColor={
                        item.color.includes("purple")
                          ? "#a855f7"
                          : item.color.includes("green")
                            ? "#22c55e"
                            : item.color.includes("yellow")
                              ? "#eab308"
                              : item.color.includes("pink")
                                ? "#ec4899"
                                : "#3b82f6"
                      }
                    />
                    <stop
                      offset="100%"
                      stopColor={
                        item.color.includes("blue") &&
                        !item.color.includes("purple")
                          ? "#06b6d4"
                          : item.color.includes("emerald")
                            ? "#10b981"
                            : item.color.includes("orange")
                              ? "#f97316"
                              : item.color.includes("rose")
                                ? "#f43f5e"
                                : "#6366f1"
                      }
                    />
                  </linearGradient>
                ))}
              </defs>
            </svg>
          </div>

          {/* Legend */}
          <div className="space-y-4">
            {tokenDistribution.map((item) => (
              <div
                key={item.label}
                className="flex items-center gap-4 glass rounded-xl p-4"
              >
                <div
                  className={`w-4 h-4 rounded-full bg-gradient-to-r ${item.color}`}
                />
                <div className="flex-grow">
                  <div className="text-white font-medium">{item.label}</div>
                  <div className="text-sm text-gray-400">
                    {((item.percentage / 100) * 1000000000).toLocaleString()}{" "}
                    DCHAT
                  </div>
                </div>
                <div className="text-2xl font-bold text-white">
                  {item.percentage}%
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Vesting Schedule */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Vesting Schedule
        </h2>
        <div className="overflow-x-auto">
          <table className="w-full glass rounded-2xl overflow-hidden">
            <thead>
              <tr className="border-b border-white/10">
                <th className="text-left p-4 text-gray-400 font-medium">
                  Category
                </th>
                <th className="text-left p-4 text-gray-400 font-medium">
                  Cliff Period
                </th>
                <th className="text-left p-4 text-gray-400 font-medium">
                  Vesting
                </th>
                <th className="text-left p-4 text-gray-400 font-medium">
                  TGE Unlock
                </th>
              </tr>
            </thead>
            <tbody>
              {vestingSchedule.map((row) => (
                <tr
                  key={row.category}
                  className="border-b border-white/5 hover:bg-white/5 transition-colors"
                >
                  <td className="p-4 text-white font-medium">{row.category}</td>
                  <td className="p-4 text-gray-300">{row.cliff}</td>
                  <td className="p-4 text-gray-300">{row.vesting}</td>
                  <td className="p-4 text-green-400 font-medium">{row.tge}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Token Utility */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Token Utility
        </h2>
        <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-6">
          <div className="glass rounded-2xl p-6 group hover:border-green-500/30 transition-colors">
            <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-green-500/20 to-emerald-500/20 flex items-center justify-center mb-4">
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
                  d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"
                />
              </svg>
            </div>
            <h3 className="text-lg font-semibold text-white mb-2">
              Message Fees
            </h3>
            <p className="text-sm text-gray-400">
              Micro-fees for message storage and delivery. ~0.0001 DCHAT per
              message.
            </p>
          </div>

          <div className="glass rounded-2xl p-6 group hover:border-purple-500/30 transition-colors">
            <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-purple-500/20 to-pink-500/20 flex items-center justify-center mb-4">
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
                  d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"
                />
              </svg>
            </div>
            <h3 className="text-lg font-semibold text-white mb-2">Staking</h3>
            <p className="text-sm text-gray-400">
              Stake DCHAT to secure the network and earn 8-12% APY rewards.
            </p>
          </div>

          <div className="glass rounded-2xl p-6 group hover:border-blue-500/30 transition-colors">
            <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-blue-500/20 to-cyan-500/20 flex items-center justify-center mb-4">
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
                  d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4"
                />
              </svg>
            </div>
            <h3 className="text-lg font-semibold text-white mb-2">
              Governance
            </h3>
            <p className="text-sm text-gray-400">
              Vote on protocol upgrades, fee structures, and validator
              elections.
            </p>
          </div>

          <div className="glass rounded-2xl p-6 group hover:border-yellow-500/30 transition-colors">
            <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-yellow-500/20 to-orange-500/20 flex items-center justify-center mb-4">
              <svg
                className="w-6 h-6 text-yellow-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M13 10V3L4 14h7v7l9-11h-7z"
                />
              </svg>
            </div>
            <h3 className="text-lg font-semibold text-white mb-2">
              Premium Features
            </h3>
            <p className="text-sm text-gray-400">
              Unlock premium channels, verified badges, and advanced analytics.
            </p>
          </div>
        </div>
      </div>

      {/* Deep Dives */}
      <div className="mb-20">
        <h2 className="text-3xl font-bold text-white mb-8 text-center">
          Economic Mechanisms
        </h2>
        <div className="space-y-4 max-w-4xl mx-auto">
          <ExpandableCard
            title="Staking Rewards & APY"
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
                  d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6"
                />
              </svg>
            }
            summary="Dynamic APY based on network participation and validator performance."
            accentColor="green"
            singleAccordion={isMobile}
            isExpanded={expandedCard === "staking"}
            onToggle={() => handleToggle("staking")}
          >
            <div className="space-y-4 text-gray-300">
              <p>
                Staking rewards are distributed through a dynamic APY model:
              </p>
              <div className="grid md:grid-cols-2 gap-4">
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">Base APY</h4>
                  <p className="text-3xl font-bold text-green-400 mb-1">8%</p>
                  <p className="text-sm text-gray-400">
                    Minimum guaranteed return for delegators
                  </p>
                </div>
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">Max APY</h4>
                  <p className="text-3xl font-bold text-green-400 mb-1">12%</p>
                  <p className="text-sm text-gray-400">
                    Achieved at 60% network staking rate
                  </p>
                </div>
              </div>
              <p className="mt-4">
                Validator commission rates range from 5-20%, with the average at
                10%. Higher-performing validators attract more delegation.
              </p>
            </div>
          </ExpandableCard>

          <ExpandableCard
            title="Fee Burn Mechanism"
            icon={
              <svg
                className="w-6 h-6 text-orange-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M17.657 18.657A8 8 0 016.343 7.343S7 9 9 10c0-2 .5-5 2.986-7C14 5 16.09 5.777 17.656 7.343A7.975 7.975 0 0120 13a7.975 7.975 0 01-2.343 5.657z"
                />
              </svg>
            }
            summary="50% of all transaction fees are permanently burned, creating deflationary pressure."
            accentColor="pink"
            singleAccordion={isMobile}
            isExpanded={expandedCard === "burn"}
            onToggle={() => handleToggle("burn")}
          >
            <div className="space-y-4 text-gray-300">
              <p>
                DCHAT implements an EIP-1559 style fee mechanism with aggressive
                burn:
              </p>
              <ul className="list-disc list-inside space-y-2 ml-4">
                <li>
                  <strong className="text-white">50% burned</strong> -
                  Permanently removed from circulation
                </li>
                <li>
                  <strong className="text-white">40% to validators</strong> -
                  Block producer rewards
                </li>
                <li>
                  <strong className="text-white">10% to treasury</strong> -
                  Funds ecosystem development
                </li>
              </ul>
              <div className="bg-orange-500/10 rounded-xl p-4 mt-4">
                <h4 className="font-semibold text-orange-400 mb-2">
                  Projected Burn Rate
                </h4>
                <p className="text-sm">
                  At current network usage, approximately 15M DCHAT is burned
                  annually, exceeding the 2% inflation rate once network
                  activity reaches 50M daily transactions.
                </p>
              </div>
            </div>
          </ExpandableCard>

          <ExpandableCard
            title="Governance Voting Power"
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
                  d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z"
                />
              </svg>
            }
            summary="Quadratic voting with stake-weighted and time-locked multipliers."
            accentColor="blue"
            singleAccordion={isMobile}
            isExpanded={expandedCard === "governance"}
            onToggle={() => handleToggle("governance")}
          >
            <div className="space-y-4 text-gray-300">
              <p>DCHAT governance uses a sophisticated voting mechanism:</p>
              <div className="space-y-3">
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    Quadratic Voting
                  </h4>
                  <p className="text-sm text-gray-400">
                    Voting power = √(staked tokens). Prevents plutocracy while
                    rewarding commitment.
                  </p>
                </div>
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    Time Lock Multiplier
                  </h4>
                  <p className="text-sm text-gray-400">
                    1x (no lock) → 2x (3 months) → 4x (1 year). Encourages
                    long-term alignment.
                  </p>
                </div>
                <div className="bg-white/5 rounded-xl p-4">
                  <h4 className="font-semibold text-white mb-2">
                    Proposal Threshold
                  </h4>
                  <p className="text-sm text-gray-400">
                    1% of circulating supply required to create proposals.
                    Prevents spam governance.
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
          href="/blockchain"
          className="inline-flex items-center gap-2 btn-primary px-8 py-4 rounded-xl font-semibold text-white"
        >
          Explore Architecture
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
