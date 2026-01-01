import Header from "@/components/Header";
import Footer from "@/components/Footer";
import DevelopersContent from "./DevelopersContent";

export const metadata = {
  title: "Developers - DChat",
  description:
    "Build on DChat - SDK documentation, API references, and smart contract examples.",
};

export default function DevelopersPage() {
  return (
    <>
      {/* Spline 3D Background Animation */}
      <div className="fixed inset-0 w-full h-full -z-10 pointer-events-none">
        <iframe
          src="https://my.spline.design/portaltotheunknown-3m2QGCwM9lNKVoEW6sukIklV/"
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
      <main className="pt-32 pb-20">
        <DevelopersContent />
      </main>
      <Footer />
    </>
  );
}
