"use client";

import { Highlight, themes } from "prism-react-renderer";

interface CodeBlockProps {
  code: string;
  language: string;
  title?: string;
}

export default function CodeBlock({ code, language, title }: CodeBlockProps) {
  const copyToClipboard = async () => {
    await navigator.clipboard.writeText(code);
  };

  return (
    <div className="rounded-xl overflow-hidden bg-[#0d1117] border border-white/10">
      {title && (
        <div className="flex items-center justify-between px-4 py-2 bg-white/5 border-b border-white/10">
          <span className="text-sm text-gray-400 font-mono">{title}</span>
          <button
            onClick={copyToClipboard}
            className="text-xs text-gray-500 hover:text-white transition-colors flex items-center gap-1"
          >
            <svg
              className="w-4 h-4"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"
              />
            </svg>
            Copy
          </button>
        </div>
      )}
      <Highlight theme={themes.nightOwl} code={code.trim()} language={language}>
        {({ className, style, tokens, getLineProps, getTokenProps }) => (
          <pre
            className={`${className} p-4 overflow-x-auto text-sm`}
            style={{ ...style, background: "transparent" }}
          >
            {tokens.map((line, i) => (
              <div key={i} {...getLineProps({ line })}>
                <span className="text-gray-600 select-none mr-4 inline-block w-8 text-right">
                  {i + 1}
                </span>
                {line.map((token, key) => (
                  <span key={key} {...getTokenProps({ token })} />
                ))}
              </div>
            ))}
          </pre>
        )}
      </Highlight>
    </div>
  );
}
