const cards = [
  {
    title: "Developer-first",
    body: "TypeScript SDK, GraphQL indexers, and starter kits for bots, wallets, and premium channels.",
    badge: "SDK",
  },
  {
    title: "Validator friendly",
    body: "Clear runbooks, Prometheus/Grafana dashboards, and automated key rotation scripts for operators.",
    badge: "Ops",
  },
  {
    title: "Extensible",
    body: "EVM-compatible Currency Chain plus hooks to emit receipts, gate rooms, or monetize premium feeds.",
    badge: "EVM",
  },
];

export default function Ecosystem() {
  return (
    <section id="ecosystem" className="py-16 lg:py-24 relative overflow-hidden">
      <div className="absolute inset-0 grid-bg opacity-[0.06]" />
      <div className="container relative mx-auto px-4 lg:px-8">
        <div className="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-4 mb-10">
          <div>
            <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass text-sm text-purple-200/80">
              <span className="w-2 h-2 rounded-full bg-purple-400" />
              Ecosystem & SDKs
            </div>
            <h2 className="heading text-3xl md:text-4xl font-semibold text-white leading-tight mt-3">
              Build on an open, verifiable stack.
            </h2>
          </div>
          <p className="text-[var(--muted)] max-w-xl text-lg">
            Fork the UI, self-host the network UI, or consume the APIs directly. Your choice, your cloud, your rules.
          </p>
        </div>

        <div className="grid gap-4 md:grid-cols-3">
          {cards.map((card) => (
            <div key={card.title} className="glass rounded-2xl p-5 border border-white/10 h-full flex flex-col gap-3">
              <div className="flex items-center gap-2 text-xs text-white/80">
                <span className="px-3 py-1 rounded-full bg-white/5 border border-white/10">{card.badge}</span>
              </div>
              <h3 className="text-xl font-semibold text-white">{card.title}</h3>
              <p className="text-[var(--muted)] text-sm leading-relaxed">{card.body}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
