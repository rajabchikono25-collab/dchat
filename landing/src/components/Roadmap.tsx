import site from "@/content/site.json";

const { roadmap } = site;

const statusConfig: Record<
  string,
  { styles: string; icon: JSX.Element; label: string }
> = {
  completed: {
    styles: "from-green-500 to-emerald-500",
    icon: (
      <svg
        className="w-4 h-4"
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
    ),
    label: "Completed",
  },
  current: {
    styles: "from-purple-500 to-blue-500",
    icon: (
      <svg
        className="w-4 h-4 animate-spin"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
        />
      </svg>
    ),
    label: "In Progress",
  },
  upcoming: {
    styles: "from-gray-500 to-gray-600",
    icon: (
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
          d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"
        />
      </svg>
    ),
    label: "Upcoming",
  },
};

export default function Roadmap() {
  return (
    <section id="roadmap" className="py-24 lg:py-32 relative overflow-hidden">
      {/* Background */}
      <div className="absolute inset-0 grid-bg opacity-20" />
      <div className="absolute top-0 right-0 w-[600px] h-[600px] bg-purple-500/10 rounded-full blur-[150px]" />
      <div className="absolute bottom-0 left-0 w-[500px] h-[500px] bg-blue-500/10 rounded-full blur-[120px]" />

      <div className="container relative mx-auto px-4 lg:px-8">
        {/* Section header */}
        <div className="text-center mb-20">
          <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass mb-6">
            <svg
              className="w-4 h-4 text-purple-400"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-3 7h3m-3 4h3m-6-4h.01M9 16h.01"
              />
            </svg>
            <span className="text-sm text-purple-400 font-medium">
              {roadmap.sectionTitle}
            </span>
          </div>
          <h2 className="text-4xl md:text-5xl lg:text-6xl font-bold mb-6">
            <span className="text-white">Our </span>
            <span className="gradient-text">Journey</span>
          </h2>
          <p className="text-gray-400 max-w-2xl mx-auto text-lg">
            Building the future of decentralized communication, one milestone at
            a time.
          </p>
        </div>

        <div className="max-w-4xl mx-auto">
          <div className="relative">
            {/* Timeline line - desktop */}
            <div className="hidden md:block absolute left-1/2 top-0 bottom-0 w-px transform -translate-x-1/2">
              <div className="w-full h-full bg-gradient-to-b from-purple-500 via-blue-500 to-gray-700" />
            </div>
            {/* Timeline line - mobile */}
            <div className="md:hidden absolute left-8 top-0 bottom-0 w-px bg-gradient-to-b from-purple-500 via-blue-500 to-gray-700" />

            {roadmap.milestones.map((milestone, index) => {
              const config = statusConfig[milestone.status];
              const isEven = index % 2 === 0;

              return (
                <div
                  key={milestone.quarter}
                  className={`relative flex items-center mb-12 ${
                    isEven ? "md:flex-row" : "md:flex-row-reverse"
                  }`}
                >
                  {/* Timeline dot */}
                  <div className="absolute left-8 md:left-1/2 transform -translate-x-1/2 z-10">
                    <div
                      className={`w-10 h-10 rounded-full bg-gradient-to-br ${config.styles} p-0.5`}
                    >
                      <div className="w-full h-full rounded-full bg-[#0a0a0f] flex items-center justify-center text-white">
                        {config.icon}
                      </div>
                    </div>
                  </div>

                  {/* Content card */}
                  <div
                    className={`ml-20 md:ml-0 md:w-[calc(50%-3rem)] group ${
                      isEven ? "md:pr-8" : "md:pl-8"
                    }`}
                  >
                    <div className="relative">
                      {/* Glow effect */}
                      <div
                        className={`absolute -inset-1 bg-gradient-to-r ${config.styles} rounded-2xl blur-lg opacity-0 group-hover:opacity-30 transition-opacity duration-300`}
                      />

                      <div className="relative glass rounded-2xl p-6 card-hover">
                        {/* Quarter badge */}
                        <div
                          className={`inline-flex items-center gap-2 px-3 py-1 rounded-full bg-gradient-to-r ${config.styles} bg-opacity-20 mb-4`}
                        >
                          <span className="text-xs font-semibold text-white">
                            {milestone.quarter}
                          </span>
                        </div>

                        <h3 className="text-xl font-semibold text-white mb-3 group-hover:gradient-text transition-all duration-300">
                          {milestone.title}
                        </h3>

                        {/* Status badge */}
                        <div className="flex items-center gap-2">
                          <div
                            className={`w-2 h-2 rounded-full bg-gradient-to-r ${config.styles}`}
                          />
                          <span className="text-sm text-gray-400">
                            {config.label}
                          </span>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}
