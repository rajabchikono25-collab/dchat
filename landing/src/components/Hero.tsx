import site from "@/content/site.json";

const { hero } = site;

export default function Hero() {
  return (
    <section className="relative min-h-screen flex items-center pt-20 overflow-hidden">
      {/* Background gradient */}
      <div className="absolute inset-0 bg-gradient-to-b from-slate-950 via-slate-900 to-slate-950" />

      {/* Animated background elements */}
      <div className="absolute inset-0 overflow-hidden">
        <div className="absolute top-1/4 left-1/4 w-96 h-96 bg-cyan-500/10 rounded-full blur-3xl animate-pulse-slow" />
        <div className="absolute bottom-1/4 right-1/4 w-96 h-96 bg-blue-500/10 rounded-full blur-3xl animate-pulse-slow delay-1000" />
      </div>

      <div className="container relative mx-auto px-4 lg:px-8 py-20 lg:py-32">
        <div className="grid lg:grid-cols-2 gap-12 lg:gap-20 items-center">
          {/* Text content */}
          <div className="text-center lg:text-left">
            <p className="text-cyan-400 font-semibold tracking-wider uppercase text-sm mb-4 animate-fade-in">
              {hero.subheadline}
            </p>
            <h1 className="text-4xl md:text-5xl lg:text-6xl font-bold mb-6 leading-tight animate-fade-in-up">
              <span className="bg-gradient-to-r from-white via-slate-200 to-slate-400 bg-clip-text text-transparent">
                {hero.headline}
              </span>
            </h1>
            <p className="text-lg text-slate-400 mb-8 max-w-xl mx-auto lg:mx-0 animate-fade-in-up delay-200">
              {hero.description}
            </p>
            <div className="flex flex-col sm:flex-row gap-4 justify-center lg:justify-start animate-fade-in-up delay-300">
              <a
                href={hero.primaryCta.href}
                className="px-8 py-4 bg-gradient-to-r from-cyan-500 to-blue-600 rounded-xl font-semibold hover:from-cyan-400 hover:to-blue-500 transition-all shadow-lg shadow-cyan-500/25 text-center"
              >
                {hero.primaryCta.label}
              </a>
              <a
                href={hero.secondaryCta.href}
                className="px-8 py-4 border border-slate-700 rounded-xl font-semibold hover:bg-slate-800 transition-all text-center"
              >
                {hero.secondaryCta.label}
              </a>
            </div>
          </div>

          {/* Animation placeholder */}
          <div className="flex justify-center lg:justify-end">
            <div className="max-w-md w-full h-[400px]">
              <svg viewBox="0 0 200 200" className="w-full h-full">
                {/* Stylized network visualization */}
                <defs>
                  <linearGradient
                    id="nodeGrad"
                    x1="0%"
                    y1="0%"
                    x2="100%"
                    y2="100%"
                  >
                    <stop
                      offset="0%"
                      style={{ stopColor: "#06b6d4", stopOpacity: 1 }}
                    />
                    <stop
                      offset="100%"
                      style={{ stopColor: "#3b82f6", stopOpacity: 1 }}
                    />
                  </linearGradient>
                </defs>
                {/* Connection lines */}
                <g stroke="url(#nodeGrad)" strokeWidth="1" opacity="0.3">
                  <line x1="100" y1="60" x2="50" y2="100" />
                  <line x1="100" y1="60" x2="150" y2="100" />
                  <line x1="50" y1="100" x2="70" y2="150" />
                  <line x1="150" y1="100" x2="130" y2="150" />
                  <line x1="70" y1="150" x2="130" y2="150" />
                  <line x1="100" y1="60" x2="100" y2="120" />
                </g>
                {/* Nodes */}
                <circle
                  cx="100"
                  cy="60"
                  r="12"
                  fill="url(#nodeGrad)"
                  opacity="0.9"
                />
                <circle
                  cx="50"
                  cy="100"
                  r="8"
                  fill="url(#nodeGrad)"
                  opacity="0.7"
                />
                <circle
                  cx="150"
                  cy="100"
                  r="8"
                  fill="url(#nodeGrad)"
                  opacity="0.7"
                />
                <circle
                  cx="70"
                  cy="150"
                  r="6"
                  fill="url(#nodeGrad)"
                  opacity="0.5"
                />
                <circle
                  cx="130"
                  cy="150"
                  r="6"
                  fill="url(#nodeGrad)"
                  opacity="0.5"
                />
                <circle
                  cx="100"
                  cy="120"
                  r="10"
                  fill="url(#nodeGrad)"
                  opacity="0.8"
                />
                {/* Lock icon in center */}
                <g transform="translate(92, 112)">
                  <rect
                    x="2"
                    y="6"
                    width="12"
                    height="10"
                    rx="1"
                    fill="white"
                    opacity="0.9"
                  />
                  <path
                    d="M4 6V4a4 4 0 018 0v2"
                    stroke="white"
                    strokeWidth="1.5"
                    fill="none"
                    opacity="0.9"
                  />
                </g>
              </svg>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
