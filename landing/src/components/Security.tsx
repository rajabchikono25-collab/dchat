"use client";

import site from "@/content/site.json";
import LottieAnimation from "./LottieAnimation";
import shieldAnimation from "@/animations/shield.json";

const { security } = site;

export default function Security() {
  return (
    <section id="security" className="py-24 lg:py-32 relative overflow-hidden">
      {/* Background */}
      <div className="absolute inset-0 bg-gradient-to-b from-transparent via-purple-500/5 to-transparent" />
      <div className="absolute top-1/2 left-0 w-[500px] h-[500px] bg-green-500/10 rounded-full blur-[150px] -translate-y-1/2" />
      <div className="absolute top-1/2 right-0 w-[400px] h-[400px] bg-blue-500/10 rounded-full blur-[120px] -translate-y-1/2" />

      <div className="container relative mx-auto px-4 lg:px-8">
        <div className="grid lg:grid-cols-2 gap-16 lg:gap-24 items-center">
          {/* Lottie Animation */}
          <div className="order-2 lg:order-1 flex justify-center">
            <div className="relative">
              {/* Outer glow ring */}
              <div className="absolute -inset-8 bg-gradient-to-r from-green-500/20 via-blue-500/20 to-purple-500/20 rounded-full blur-3xl animate-pulse-glow" />

              {/* Main animation container */}
              <div className="relative glass-strong rounded-3xl p-8 overflow-hidden">
                {/* Inner gradient */}
                <div className="absolute inset-0 bg-gradient-to-br from-green-500/5 to-blue-500/5" />

                <LottieAnimation
                  animationData={shieldAnimation}
                  className="w-[300px] h-[300px] relative z-10"
                />

                {/* Floating security badges */}
                <div className="absolute top-4 right-4 glass rounded-lg px-3 py-1.5 flex items-center gap-2 animate-float">
                  <div className="w-2 h-2 rounded-full bg-green-400 animate-pulse" />
                  <span className="text-xs text-green-400 font-medium">
                    Protected
                  </span>
                </div>
                <div
                  className="absolute bottom-4 left-4 glass rounded-lg px-3 py-1.5 flex items-center gap-2 animate-float"
                  style={{ animationDelay: "1s" }}
                >
                  <svg
                    className="w-3 h-3 text-blue-400"
                    fill="currentColor"
                    viewBox="0 0 20 20"
                  >
                    <path
                      fillRule="evenodd"
                      d="M2.166 4.999A11.954 11.954 0 0010 1.944 11.954 11.954 0 0017.834 5c.11.65.166 1.32.166 2.001 0 5.225-3.34 9.67-8 11.317C5.34 16.67 2 12.225 2 7c0-.682.057-1.35.166-2.001zm11.541 3.708a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
                      clipRule="evenodd"
                    />
                  </svg>
                  <span className="text-xs text-blue-400 font-medium">
                    E2E Encrypted
                  </span>
                </div>
              </div>
            </div>
          </div>

          {/* Text content */}
          <div className="order-1 lg:order-2">
            <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass mb-6">
              <svg
                className="w-4 h-4 text-green-400"
                fill="currentColor"
                viewBox="0 0 20 20"
              >
                <path
                  fillRule="evenodd"
                  d="M2.166 4.999A11.954 11.954 0 0010 1.944 11.954 11.954 0 0017.834 5c.11.65.166 1.32.166 2.001 0 5.225-3.34 9.67-8 11.317C5.34 16.67 2 12.225 2 7c0-.682.057-1.35.166-2.001zm11.541 3.708a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
                  clipRule="evenodd"
                />
              </svg>
              <span className="text-sm text-green-400 font-medium">
                {security.sectionTitle}
              </span>
            </div>

            <h2 className="text-4xl md:text-5xl lg:text-6xl font-bold mb-6">
              <span className="text-white">
                {security.headline.split(" ").slice(0, -2).join(" ")}{" "}
              </span>
              <span className="gradient-text-green">
                {security.headline.split(" ").slice(-2).join(" ")}
              </span>
            </h2>

            <p className="text-lg text-gray-400 mb-10 leading-relaxed">
              {security.description}
            </p>

            {/* Stats */}
            <div className="grid grid-cols-3 gap-4">
              {security.stats.map((stat, index) => (
                <div key={stat.label} className="relative group cursor-default">
                  <div className="absolute inset-0 bg-gradient-to-br from-green-500/20 to-blue-500/20 rounded-2xl blur-xl opacity-0 group-hover:opacity-100 transition-opacity duration-300" />
                  <div className="relative glass rounded-2xl p-5 text-center card-hover">
                    <div className="text-2xl md:text-3xl font-bold gradient-text-green mb-2">
                      {stat.value}
                    </div>
                    <div className="text-xs md:text-sm text-gray-500 group-hover:text-gray-400 transition-colors">
                      {stat.label}
                    </div>
                  </div>
                </div>
              ))}
            </div>

            {/* Security features list */}
            <div className="mt-10 grid grid-cols-2 gap-4">
              {[
                "Zero-knowledge proofs",
                "Decentralized nodes",
                "Military-grade encryption",
                "No metadata leaks",
              ].map((feature, index) => (
                <div key={index} className="flex items-center gap-3">
                  <div className="w-5 h-5 rounded-full bg-gradient-to-br from-green-500 to-blue-500 flex items-center justify-center flex-shrink-0">
                    <svg
                      className="w-3 h-3 text-white"
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth={3}
                        d="M5 13l4 4L19 7"
                      />
                    </svg>
                  </div>
                  <span className="text-sm text-gray-400">{feature}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
