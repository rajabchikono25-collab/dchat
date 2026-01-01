"use client";

import { useState, ReactNode } from "react";

interface ExpandableCardProps {
  title: string;
  icon: ReactNode;
  summary: string;
  children: ReactNode;
  defaultExpanded?: boolean;
  accentColor?: string;
  singleAccordion?: boolean;
  isExpanded?: boolean;
  onToggle?: () => void;
}

export default function ExpandableCard({
  title,
  icon,
  summary,
  children,
  defaultExpanded = false,
  accentColor = "purple",
  singleAccordion = false,
  isExpanded: controlledExpanded,
  onToggle,
}: ExpandableCardProps) {
  const [internalExpanded, setInternalExpanded] = useState(defaultExpanded);

  const isExpanded = singleAccordion ? controlledExpanded : internalExpanded;
  const toggleExpand = () => {
    if (singleAccordion && onToggle) {
      onToggle();
    } else {
      setInternalExpanded(!internalExpanded);
    }
  };

  const colorClasses: Record<
    string,
    { gradient: string; hover: string; text: string }
  > = {
    purple: {
      gradient: "from-purple-500/20 to-blue-500/20",
      hover: "hover:border-purple-500/30",
      text: "text-purple-400",
    },
    blue: {
      gradient: "from-blue-500/20 to-cyan-500/20",
      hover: "hover:border-blue-500/30",
      text: "text-blue-400",
    },
    green: {
      gradient: "from-green-500/20 to-emerald-500/20",
      hover: "hover:border-green-500/30",
      text: "text-green-400",
    },
    pink: {
      gradient: "from-pink-500/20 to-rose-500/20",
      hover: "hover:border-pink-500/30",
      text: "text-pink-400",
    },
  };

  const colors = colorClasses[accentColor] || colorClasses.purple;

  return (
    <div
      className={`glass rounded-2xl overflow-hidden transition-all duration-300 ${colors.hover}`}
    >
      <button
        onClick={toggleExpand}
        className="w-full p-6 flex items-start gap-4 text-left"
        aria-expanded={isExpanded}
      >
        <div
          className={`flex-shrink-0 w-12 h-12 rounded-xl bg-gradient-to-br ${colors.gradient} flex items-center justify-center`}
        >
          {icon}
        </div>
        <div className="flex-grow min-w-0">
          <h3 className="text-lg font-semibold text-white mb-1">{title}</h3>
          <p className="text-gray-400 text-sm line-clamp-2">{summary}</p>
        </div>
        <div
          className={`flex-shrink-0 transition-transform duration-300 ${isExpanded ? "rotate-180" : ""}`}
        >
          <svg
            className="w-5 h-5 text-gray-400"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M19 9l-7 7-7-7"
            />
          </svg>
        </div>
      </button>

      <div
        className={`overflow-hidden transition-all duration-300 ${
          isExpanded ? "max-h-[2000px] opacity-100" : "max-h-0 opacity-0"
        }`}
      >
        <div className="px-6 pb-6 pt-2 border-t border-white/5">{children}</div>
      </div>
    </div>
  );
}
