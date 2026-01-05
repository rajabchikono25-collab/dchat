const outcomes = [
  {
    title: "Ship faster",
    description: "SDKs, starter UI, and scripts so you can drop messaging into your stack in days, not weeks.",
    stat: "3x",
    tag: "Faster build cycles",
  },
  {
    title: "Prove privacy",
    description: "Zero-knowledge metadata, end-to-end encryption, and verifiable delivery receipts baked into the protocol.",
    stat: "0 leaks",
    tag: "Metadata minimized",
  },
  {
    title: "Stay resilient",
    description: "Multi-region validators, shard-aware routing, and BFT consensus keep messages flowing under load.",
    stat: "99.9%",
    tag: "Network uptime",
  },
];

export default function Outcomes() {
  return (
    <section className="py-16 lg:py-24">
      <div className="container mx-auto px-4 lg:px-8">
        <div className="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-6 mb-10">
          <div className="space-y-3 max-w-2xl">
            <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass text-sm text-blue-200/80">
              <span className="w-2 h-2 rounded-full bg-blue-300" />
              Outcomes, not features
            </div>
            <h2 className="heading text-3xl md:text-4xl font-semibold text-white leading-tight">
              What teams actually get with DChat.
            </h2>
            <p className="text-[var(--muted)] text-lg">
              Less ceremony, fewer moving parts, more verifiable privacy.
            </p>
          </div>
          <div className="text-sm text-[var(--muted)]">
            Built for teams shipping AI assistants, secure communities, and on-chain coordination tools.
          </div>
        </div>

        <div className="grid gap-4 md:grid-cols-3">
          {outcomes.map((item) => (
            <div key={item.title} className="glass rounded-2xl p-5 border border-white/10 h-full flex flex-col gap-4">
              <div className="flex items-center justify-between">
                <div className="text-xs px-3 py-1 rounded-full bg-white/5 text-white/80 border border-white/10">
                  {item.tag}
                </div>
                <span className="text-2xl font-semibold gradient-text-purple">{item.stat}</span>
              </div>
              <div className="space-y-2">
                <h3 className="text-xl font-semibold text-white">{item.title}</h3>
                <p className="text-[var(--muted)] text-sm leading-relaxed">{item.description}</p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
