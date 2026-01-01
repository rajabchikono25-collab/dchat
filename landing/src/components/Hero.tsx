"use client";

import site from "@/content/site.json";
import Image from "next/image";

const { hero } = site;

export default function Hero() {
  return (
    <section className="relative min-h-screen flex items-center pt-32 overflow-hidden">
      {/* Grid background */}
      <div className="absolute inset-0 grid-bg opacity-50" />

      {/* Animated background orbs */}
      <div className="absolute inset-0 overflow-hidden">
        <div className="absolute top-20 left-10 w-[600px] h-[600px] bg-purple-500/20 rounded-full blur-[120px] animate-pulse-glow" />
        <div className="absolute bottom-20 right-10 w-[500px] h-[500px] bg-blue-500/20 rounded-full blur-[100px] animate-pulse-glow delay-1000" />
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[800px] h-[800px] bg-purple-600/10 rounded-full blur-[150px]" />
      </div>

      {/* Floating decorative elements */}
      <div className="absolute top-40 right-20 w-20 h-20 border border-purple-500/20 rounded-2xl rotate-12 animate-float-slow hidden lg:block" />
      <div className="absolute bottom-40 left-20 w-16 h-16 border border-blue-500/20 rounded-full animate-float hidden lg:block" />
      <div className="absolute top-60 left-[15%] w-3 h-3 bg-purple-500 rounded-full animate-pulse-glow hidden lg:block" />
      <div className="absolute bottom-60 right-[15%] w-2 h-2 bg-blue-500 rounded-full animate-pulse-glow hidden lg:block" />

      <div className="container relative mx-auto px-4 lg:px-8 py-20 lg:py-32">
        <div className="grid lg:grid-cols-2 gap-12 lg:gap-20 items-center">
          {/* Text content */}
          <div className="text-center lg:text-left">
            {/* Badge */}
            <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass mb-8 animate-fade-in">
              <span className="w-2 h-2 bg-green-500 rounded-full animate-pulse" />
              <span className="text-sm text-gray-400">{hero.subheadline}</span>
            </div>

            <h1 className="text-5xl md:text-6xl lg:text-7xl font-bold mb-8 leading-[1.1] animate-fade-in-up">
              <span className="text-white">Messaging</span>
              <br />
              <span className="gradient-text">Without Boundaries</span>
            </h1>

            <p className="text-lg md:text-xl text-gray-400 mb-10 max-w-xl mx-auto lg:mx-0 leading-relaxed animate-fade-in-up delay-200">
              {hero.description}
            </p>

            <div className="flex flex-col sm:flex-row gap-4 justify-center lg:justify-start animate-fade-in-up delay-300">
              <a
                href={hero.primaryCta.href}
                className="btn-primary px-8 py-4 rounded-xl font-semibold text-white text-center inline-flex items-center justify-center gap-2"
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
                className="px-8 py-4 glass rounded-xl font-semibold text-white hover:bg-white/10 transition-all text-center inline-flex items-center justify-center gap-2"
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

            {/* Stats row */}
            <div className="flex flex-wrap gap-8 mt-12 justify-center lg:justify-start animate-fade-in-up delay-400">
              <div className="text-center lg:text-left">
                <div className="text-3xl font-bold gradient-text-purple">
                  100K+
                </div>
                <div className="text-sm text-gray-500">Active Users</div>
              </div>
              <div className="text-center lg:text-left">
                <div className="text-3xl font-bold gradient-text-green">
                  99.9%
                </div>
                <div className="text-sm text-gray-500">Uptime</div>
              </div>
              <div className="text-center lg:text-left">
                <div className="text-3xl font-bold gradient-text">50M+</div>
                <div className="text-sm text-gray-500">Messages Sent</div>
              </div>
            </div>
          </div>

          {/* Hero Image with glassmorphism card */}
          <div className="flex justify-center lg:justify-end animate-fade-in-right delay-300">
            <div className="relative">
              {/* Glow effect */}
              <div className="absolute -inset-4 bg-gradient-to-r from-purple-500/30 via-blue-500/30 to-purple-500/30 rounded-3xl blur-2xl animate-pulse-glow" />

              {/* Main card */}
              <div className="relative glass-strong rounded-3xl p-8 w-full max-w-lg">
                <div className="relative h-[400px] flex items-center justify-center">
                  <Image
                    src="/images/network-hero.svg"
                    alt="Decentralized Network"
                    width={450}
                    height={450}
                    className="w-full h-auto drop-shadow-2xl"
                    priority
                  />
                </div>

                {/* Floating mini cards */}
                <div className="absolute -top-4 -right-4 glass rounded-xl px-4 py-2 animate-float">
                  <div className="flex items-center gap-2">
                    <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-green-400 to-green-600 flex items-center justify-center">
                      <svg
                        className="w-4 h-4 text-white"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M5 13l4 4L19 7"
                        />
                      </svg>
                    </div>
                    <div>
                      <div className="text-xs text-gray-400">Encrypted</div>
                      <div className="text-sm font-semibold text-white">
                        256-bit AES
                      </div>
                    </div>
                  </div>
                </div>

                <div className="absolute -bottom-4 -left-4 glass rounded-xl px-4 py-2 animate-float-slow">
                  <div className="flex items-center gap-2">
                    <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-400 to-purple-600 flex items-center justify-center">
                      <svg
                        className="w-4 h-4 text-white"
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
                    </div>
                    <div>
                      <div className="text-xs text-gray-400">Status</div>
                      <div className="text-sm font-semibold text-white">
                        Decentralized
                      </div>
                    </div>
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
