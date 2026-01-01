import site from "@/content/site.json";

const { security } = site;

export default function Security() {
  return (
    <section id="security" className="py-20 lg:py-32 bg-slate-900/50">
      <div className="container mx-auto px-4 lg:px-8">
        <div className="grid lg:grid-cols-2 gap-12 lg:gap-20 items-center">
          {/* Animation placeholder */}
          <div className="order-2 lg:order-1 flex justify-center">
            <div className="max-w-sm w-full h-[300px]">
              <svg viewBox="0 0 200 200" className="w-full h-full">
                <defs>
                  <linearGradient
                    id="shieldGrad"
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
                {/* Shield shape */}
                <path
                  d="M100 20 L160 45 L160 95 C160 135 130 165 100 180 C70 165 40 135 40 95 L40 45 Z"
                  fill="url(#shieldGrad)"
                  opacity="0.2"
                  stroke="url(#shieldGrad)"
                  strokeWidth="2"
                />
                {/* Inner shield */}
                <path
                  d="M100 40 L140 55 L140 90 C140 120 120 145 100 155 C80 145 60 120 60 90 L60 55 Z"
                  fill="url(#shieldGrad)"
                  opacity="0.4"
                />
                {/* Checkmark */}
                <path
                  d="M80 95 L95 110 L120 80"
                  stroke="white"
                  strokeWidth="6"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  fill="none"
                />
              </svg>
            </div>
          </div>

          {/* Text content */}
          <div className="order-1 lg:order-2">
            <p className="text-cyan-400 font-semibold tracking-wider uppercase text-sm mb-4">
              {security.sectionTitle}
            </p>
            <h2 className="text-3xl md:text-4xl font-bold mb-6">
              <span className="bg-gradient-to-r from-white via-slate-200 to-slate-400 bg-clip-text text-transparent">
                {security.headline}
              </span>
            </h2>
            <p className="text-lg text-slate-400 mb-8">
              {security.description}
            </p>

            {/* Stats */}
            <div className="grid grid-cols-3 gap-4">
              {security.stats.map((stat) => (
                <div
                  key={stat.label}
                  className="text-center p-4 bg-slate-800/50 rounded-xl border border-slate-700/50"
                >
                  <div className="text-xl md:text-2xl font-bold text-cyan-400 mb-1">
                    {stat.value}
                  </div>
                  <div className="text-xs md:text-sm text-slate-500">
                    {stat.label}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
