import site from "@/content/site.json";

const { cta } = site;

export default function CTA() {
  return (
    <section id="get-started" className="py-20 lg:py-32">
      <div className="container mx-auto px-4 lg:px-8">
        <div className="relative overflow-hidden rounded-3xl bg-gradient-to-br from-cyan-600 via-blue-600 to-blue-700 p-8 md:p-12 lg:p-16">
          {/* Background decoration */}
          <div className="absolute inset-0 overflow-hidden">
            <div className="absolute -top-1/2 -right-1/2 w-full h-full bg-gradient-to-bl from-white/10 to-transparent rounded-full" />
            <div className="absolute -bottom-1/2 -left-1/2 w-full h-full bg-gradient-to-tr from-white/5 to-transparent rounded-full" />
          </div>

          <div className="relative text-center max-w-2xl mx-auto">
            <h2 className="text-3xl md:text-4xl lg:text-5xl font-bold mb-4 text-white">
              {cta.headline}
            </h2>
            <p className="text-lg text-cyan-100/80 mb-8">{cta.description}</p>
            <div className="flex flex-col sm:flex-row gap-4 justify-center">
              <a
                href={cta.primaryButton.href}
                className="px-8 py-4 bg-white text-blue-600 rounded-xl font-semibold hover:bg-cyan-50 transition-all shadow-lg text-center"
              >
                {cta.primaryButton.label}
              </a>
              <a
                href={cta.secondaryButton.href}
                className="px-8 py-4 border-2 border-white/30 rounded-xl font-semibold hover:bg-white/10 transition-all text-center text-white"
              >
                {cta.secondaryButton.label}
              </a>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
