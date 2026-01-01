import site from "@/content/site.json";

const { footer } = site;

export default function Footer() {
  return (
    <footer className="py-12 border-t border-slate-800">
      <div className="container mx-auto px-4 lg:px-8">
        <div className="flex flex-col md:flex-row items-center justify-between gap-6">
          {/* Logo and tagline */}
          <div className="text-center md:text-left">
            <a
              href="/"
              className="text-xl font-bold bg-gradient-to-r from-cyan-400 to-blue-500 bg-clip-text text-transparent"
            >
              {footer.logo}
            </a>
            <p className="text-slate-500 text-sm mt-1">{footer.tagline}</p>
          </div>

          {/* Links */}
          <div className="flex flex-wrap justify-center gap-6">
            {footer.links.map((link) => (
              <a
                key={link.label}
                href={link.href}
                className="text-slate-400 hover:text-white transition-colors text-sm"
                target={link.href.startsWith("http") ? "_blank" : undefined}
                rel={
                  link.href.startsWith("http")
                    ? "noopener noreferrer"
                    : undefined
                }
              >
                {link.label}
              </a>
            ))}
          </div>

          {/* Copyright */}
          <p className="text-slate-600 text-sm">{footer.copyright}</p>
        </div>
      </div>
    </footer>
  );
}
