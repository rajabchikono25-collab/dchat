import site from "@/content/site.json";

const { roadmap } = site;

const statusStyles: Record<string, string> = {
  completed: "bg-green-500/20 text-green-400 border-green-500/50",
  current: "bg-cyan-500/20 text-cyan-400 border-cyan-500/50",
  upcoming: "bg-slate-700/50 text-slate-400 border-slate-600/50",
};

const statusLabels: Record<string, string> = {
  completed: "✓ Completed",
  current: "● In Progress",
  upcoming: "○ Upcoming",
};

export default function Roadmap() {
  return (
    <section id="roadmap" className="py-20 lg:py-32">
      <div className="container mx-auto px-4 lg:px-8">
        <div className="text-center mb-16">
          <h2 className="text-3xl md:text-4xl font-bold mb-4">
            <span className="bg-gradient-to-r from-cyan-400 to-blue-500 bg-clip-text text-transparent">
              {roadmap.sectionTitle}
            </span>
          </h2>
          <p className="text-slate-400 max-w-2xl mx-auto">
            Our journey to decentralized communication.
          </p>
        </div>

        <div className="max-w-3xl mx-auto">
          <div className="relative">
            {/* Timeline line */}
            <div className="absolute left-4 md:left-1/2 top-0 bottom-0 w-0.5 bg-gradient-to-b from-cyan-500 via-blue-500 to-slate-700 transform md:-translate-x-1/2" />

            {roadmap.milestones.map((milestone, index) => (
              <div
                key={milestone.quarter}
                className={`relative flex items-start gap-6 mb-8 ${
                  index % 2 === 0 ? "md:flex-row" : "md:flex-row-reverse"
                }`}
              >
                {/* Timeline dot */}
                <div className="absolute left-4 md:left-1/2 w-4 h-4 rounded-full bg-gradient-to-br from-cyan-500 to-blue-600 border-4 border-slate-950 transform -translate-x-1/2 z-10" />

                {/* Content card */}
                <div
                  className={`ml-12 md:ml-0 md:w-[calc(50%-2rem)] p-6 bg-slate-800/50 rounded-xl border ${
                    statusStyles[milestone.status]
                  } ${
                    index % 2 === 0
                      ? "md:mr-auto md:text-right"
                      : "md:ml-auto md:text-left"
                  }`}
                >
                  <span className="text-sm text-slate-500">
                    {milestone.quarter}
                  </span>
                  <h3 className="text-lg font-semibold text-white mt-1 mb-2">
                    {milestone.title}
                  </h3>
                  <span
                    className={`inline-block text-xs px-2 py-1 rounded-full ${
                      statusStyles[milestone.status]
                    }`}
                  >
                    {statusLabels[milestone.status]}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
