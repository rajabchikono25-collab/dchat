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
      <Header />
      <main className="pt-32 pb-20">
        <DevelopersContent />
      </main>
      <Footer />
    </>
  );
}
