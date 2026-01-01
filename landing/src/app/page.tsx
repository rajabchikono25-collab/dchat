import Header from "@/components/Header";
import Hero from "@/components/Hero";
import Features from "@/components/Features";
import HowItWorks from "@/components/HowItWorks";
import Security from "@/components/Security";
import Roadmap from "@/components/Roadmap";
import CTA from "@/components/CTA";
import Footer from "@/components/Footer";

export default function Home() {
  return (
    <>
      {/* Spline 3D Background Animation */}
      <div className="fixed inset-0 w-full h-full -z-10 pointer-events-none">
        <iframe
          src="https://my.spline.design/claritystream-fnk0leBHaC4ASE8ZjPf7OqL8/"
          frameBorder="0"
          width="100%"
          height="100%"
          className="absolute inset-0 w-full h-full"
          style={{ pointerEvents: "none" }}
          title="Background Animation"
        />
        {/* Overlay to blend with content */}
        <div className="absolute inset-0 bg-[#0a0a0f]/70" />
      </div>

      <Header />
      <main>
        <Hero />
        <Features />
        <HowItWorks />
        <Security />
        <Roadmap />
        <CTA />
      </main>
      <Footer />
    </>
  );
}
