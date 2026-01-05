const buttons = [
  {
    label: "Download for macOS",
    href: "#",
    icon: (
      <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
        <path d="M12 0c-1.3 0-2.6.4-3.7 1.1-1.1.7-2 1.8-2.6 3-1.3 2.6-1 6 .8 8.5.6.9 1.5 2 2.6 2 1 0 1.3-.7 2.6-.7 1.2 0 1.5.7 2.6.7 1.1 0 1.9-1.1 2.6-2 .7-1 1-2 .9-2.1-1.6-.1-3-.9-3.9-2-.8-1-1.1-2.4-.9-3.7.2-1.2 1-2.3 2-3.1-1.2-1.3-2.8-2.1-4.3-2.1zm2.4 3.4c-.5.6-.9 1.4-1.1 2.2-.1.8 0 1.6.5 2.3.5.6 1.3 1 2.2 1.1.1-.8 0-1.6-.5-2.3-.5-.6-1.3-1-2.1-1.1z" />
      </svg>
    ),
  },
  {
    label: "Get it on Windows",
    href: "#",
    icon: (
      <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
        <path d="M1 4l9-2v9H1V4zm0 16l9 2v-9H1v7zm11-7h11V2l-11 2v9zm0 2v9l11-2v-7H12z" />
      </svg>
    ),
  },
  {
    label: "CLI + Docker",
    href: "https://github.com/dchat",
    icon: (
      <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
        <path d="M21 13c-.6 0-1 .4-1 1v4H4V6h8c.6 0 1-.4 1-1s-.4-1-1-1H3C2.4 4 2 4.4 2 5v14c0 .6.4 1 1 1h18c.6 0 1-.4 1-1v-5c0-.6-.4-1-1-1z" />
        <path d="M23 3h-6c-.6 0-1 .4-1 1v6c0 .6.4 1 1 1h6c.6 0 1-.4 1-1V4c0-.6-.4-1-1-1zm-1 6h-4V5h4v4z" />
      </svg>
    ),
  },
];

export default function Download() {
  return (
    <section id="download" className="py-16 lg:py-24 relative overflow-hidden">
      <div className="absolute inset-0 grid-bg opacity-[0.08]" />
      <div className="container relative mx-auto px-4 lg:px-8">
        <div className="surface p-8 lg:p-10 shadow-2xl relative overflow-hidden">
          <div className="absolute -top-20 -right-10 w-64 h-64 bg-[rgba(124,141,247,0.14)] blur-[90px]" />
          <div className="absolute -bottom-24 -left-16 w-72 h-72 bg-[rgba(79,209,197,0.12)] blur-[90px]" />

          <div className="relative grid gap-6 lg:grid-cols-[1.4fr,1fr] items-center">
            <div className="space-y-4">
              <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full glass text-sm text-white/80">
                <span className="w-2 h-2 rounded-full bg-emerald-400" />
                Ready to run
              </div>
              <h2 className="heading text-3xl md:text-4xl font-semibold text-white leading-tight">
                Grab the desktop client or automate with CLI.
              </h2>
              <p className="text-[var(--muted)] text-lg leading-relaxed max-w-2xl">
                Ship faster with pre-built binaries, Docker images, and a CLI that pairs with our SDKs. No gatekeeping, no central accounts.
              </p>

              <div className="flex flex-wrap gap-3 pt-2">
                {buttons.map((btn) => (
                  <a
                    key={btn.label}
                    href={btn.href}
                    className="inline-flex items-center gap-2 px-4 py-3 rounded-xl bg-white text-gray-900 font-semibold shadow-lg hover:shadow-white/20 transition-transform hover:-translate-y-0.5"
                  >
                    {btn.icon}
                    {btn.label}
                  </a>
                ))}
              </div>

              <div className="flex flex-wrap gap-4 text-sm text-white/80 pt-4">
                <div className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-emerald-400" />
                  Signed binaries
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-emerald-400" />
                  SHA-256 checksums
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-emerald-400" />
                  Auto-update channel
                </div>
              </div>
            </div>

            <div className="relative">
              <div className="glass rounded-2xl p-6 border border-white/10 shadow-xl">
                <div className="text-xs text-[var(--muted)] mb-3">Quickstart</div>
                <pre className="text-sm text-white bg-black/40 border border-white/10 rounded-xl p-4 overflow-x-auto">
{`curl -sL https://dchat.sh/install | bash

dchat auth init --keypair ./id.json
dchat send --channel genesis --message "Hello, world"`}
                </pre>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
