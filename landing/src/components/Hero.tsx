"use client";

import site from "@/content/site.json";
import Image from "next/image";

const { hero } = site;

export default function Hero() {
  return (
    <section className="relative overflow-hidden pt-28 pb-20 lg:pb-28">
      <div className="absolute inset-0 grid-bg opacity-[0.08]" />
      <div className="absolute inset-x-0 top-10 mx-auto h-64 w-3/4 max-w-5xl bg-gradient-to-r from-[rgba(79,209,197,0.15)] via-[rgba(106,90,205,0.18)] to-[rgba(245,158,11,0.14)] blur-[120px]" />

      <div className="container relative mx-auto px-4 lg:px-8">
        <div className="flex flex-col gap-12 lg:grid lg:grid-cols-2 lg:items-center lg:gap-14">
          <div className="order-2 lg:order-1 space-y-6 text-center lg:text-left">
            <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass text-sm text-emerald-200/80">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
              {hero.subheadline}
            </div>

            <h1 className="heading text-4xl md:text-5xl lg:text-6xl font-semibold leading-[1.05]">
              <span className="text-white">Messaging without limits.</span>
              <br />
              <span className="gradient-text">Owned by the people.</span>
            </h1>

            <p className="text-lg md:text-xl text-[var(--muted)] leading-relaxed max-w-xl mx-auto lg:mx-0">
              {hero.description}
            </p>

            <div className="flex flex-col sm:flex-row items-center justify-center lg:justify-start gap-3">
              <a
                href={hero.primaryCta.href}
                className="btn-primary px-7 py-3.5 rounded-xl font-semibold text-white text-center inline-flex items-center justify-center gap-2 shadow-lg shadow-[rgba(79,209,197,0.25)]"
              >
                {hero.primaryCta.label}
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
                href={hero.secondaryCta.href}
                className="px-7 py-3.5 rounded-xl font-semibold text-white/90 border border-white/10 hover:border-white/30 transition-all text-center inline-flex items-center justify-center gap-2"
              >
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
                    d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z"
                  />
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                  />
                </svg>
                {hero.secondaryCta.label}
              </a>
            </div>

            <div className="grid grid-cols-3 gap-4 pt-6 text-left">
              {[{
                label: "Active users",
                value: "100K+",
                accent: "gradient-text-purple",
              },
              {
                label: "Network uptime",
                value: "99.9%",
                accent: "gradient-text-green",
              },
              {
                label: "Messages sent",
                value: "50M+",
                accent: "gradient-text",
              }].map((item) => (
                <div key={item.label} className="glass rounded-2xl px-4 py-3 text-left">
                  <div className={`text-xl font-semibold ${item.accent}`}>{item.value}</div>
                  <div className="text-xs uppercase tracking-wide text-[var(--muted)]">
                    {item.label}
                  </div>
                </div>
              ))}
            </div>

            <div className="flex flex-wrap gap-3 pt-4 justify-center lg:justify-start text-sm text-[var(--muted)]">
              {["Open-source", "Multi-region validators", "Encrypted by default"].map((pill) => (
                <span key={pill} className="px-3 py-1 rounded-full border border-white/10 bg-white/5">
                  {pill}
                </span>
              ))}
            </div>
          </div>

          <div className="order-1 lg:order-2">
            <div className="relative">
              <div className="absolute inset-0 blur-[70px] bg-gradient-to-br from-[rgba(79,209,197,0.15)] via-[rgba(124,141,247,0.2)] to-[rgba(245,158,11,0.15)]" />
              <div className="relative surface shadow-2xl p-6 lg:p-8">
                <div className="flex items-center justify-between mb-4">
                  <div className="flex items-center gap-2 text-sm text-[var(--muted)]">
                    <span className="w-2 h-2 rounded-full bg-emerald-400" />
                    Live validator mesh
                  </div>
                  <span className="text-xs px-3 py-1 rounded-full bg-white/5 border border-white/10 text-white/80">
                    Finality &lt; 3s
                  </span>
                </div>
                <div className="rounded-2xl bg-gradient-to-br from-[#0f172a] to-[#0b1323] border border-white/5 p-6">
                  <Image
                    src="/images/network-hero.svg"
                    alt="Decentralized Network"
                    width={480}
                    height={360}
                    className="w-full h-auto"
                    priority
                  />
                </div>
                <div className="mt-4 grid grid-cols-2 gap-3 text-sm text-white/80">
                  <div className="glass rounded-xl p-3">
                    <div className="text-[var(--muted)]">Encryption</div>
                    <div className="font-semibold">X25519 + AES-256-GCM</div>
                  </div>
                  <div className="glass rounded-xl p-3">
                    <div className="text-[var(--muted)]">Consensus</div>
                    <div className="font-semibold">Tendermint BFT · 21 validators</div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
