export default function ProductPreview() {
  return (
    <section className="py-20 lg:py-28 relative overflow-hidden">
      <div className="absolute inset-0 grid-bg opacity-[0.06]" />
      <div className="container relative mx-auto px-4 lg:px-8">
        <div className="flex flex-col lg:flex-row items-start gap-10 lg:gap-16">
          <div className="max-w-xl space-y-4">
            <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass text-sm text-amber-200/80">
              <span className="w-2 h-2 rounded-full bg-amber-400 animate-pulse" />
              Product preview
            </div>
            <h2 className="heading text-3xl md:text-4xl lg:text-5xl font-semibold text-white leading-tight">
              See the network-ready messenger UI we ship.
            </h2>
            <p className="text-lg text-[var(--muted)] leading-relaxed">
              Opinionated defaults for encrypted chats, channel governance, and validator transparency. Everything is open-source so you can fork, theme, and embed it in your own apps.
            </p>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-sm text-white/80">
              {["On-device keys · no cloud escrow", "Shard-aware delivery · &lt;500ms", "Validator proofs pinned to every thread", "Human-friendly message receipts"].map(
                (item) => (
                  <div key={item} className="glass rounded-xl px-4 py-3 border border-white/10">
                    {item}
                  </div>
                ),
              )}
            </div>
          </div>

          <div className="flex-1 w-full">
            <div className="relative surface p-6 lg:p-8 shadow-2xl">
              <div className="absolute -top-10 right-6 text-xs px-3 py-2 rounded-full bg-white/10 border border-white/15 text-white/80">
                Build once · theme fast
              </div>
              <div className="rounded-2xl bg-gradient-to-br from-[#0f1628] via-[#0b1220] to-[#0c172a] border border-white/5 p-6">
                <div className="flex items-center justify-between text-xs text-[var(--muted)] mb-4">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-emerald-400" />
                    dchat://channel/cosmos-security
                  </div>
                  <span className="px-3 py-1 rounded-full bg-white/5 text-white/80 border border-white/10">
                    zk metadata off
                  </span>
                </div>

                <div className="space-y-4 text-sm text-white/90">
                  <div className="glass rounded-xl p-4 border border-white/5">
                    <div className="flex items-center justify-between mb-2">
                      <div className="font-semibold">Validator Ops</div>
                      <span className="text-[var(--muted)]">08:24 UTC</span>
                    </div>
                    <p className="text-[var(--muted)]">
                      &quot;Shard 12 rotated keys. Attestation hash pinned. Delivery latency holding at 412ms p95.&quot;
                    </p>
                  </div>

                  <div className="glass rounded-xl p-4 border border-white/5">
                    <div className="flex items-center gap-2 text-emerald-200">
                      <span className="w-2 h-2 rounded-full bg-emerald-400" />
                      Encrypted channel
                    </div>
                    <p className="mt-2 text-[var(--muted)]">Messages stored as Merkle leaves. Audit any hop with a single click.</p>
                  </div>

                  <div className="grid grid-cols-2 gap-3 text-xs">
                    <div className="glass rounded-xl p-3 border border-white/5">
                      <div className="text-[var(--muted)]">Delivery p95</div>
                      <div className="text-white font-semibold">&lt; 0.5s</div>
                    </div>
                    <div className="glass rounded-xl p-3 border border-white/5">
                      <div className="text-[var(--muted)]">Validator set</div>
                      <div className="text-white font-semibold">21 · multi-region</div>
                    </div>
                  </div>
                </div>
              </div>

              <div className="absolute -bottom-8 -left-8 hidden md:block">
                <div className="glass rounded-2xl px-4 py-3 text-xs text-[var(--muted)] border border-white/10 shadow-lg">
                  Tailwind + React server components · ready for theming
                </div>
              </div>

              <div className="absolute -top-6 -left-6 hidden md:block">
                <div className="px-3 py-2 rounded-full bg-emerald-500/10 text-emerald-200 text-xs border border-emerald-400/30">
                  Self-host or deploy to Vercel
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
