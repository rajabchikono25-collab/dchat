import Header from "@/components/Header";
import Footer from "@/components/Footer";
import TokenomicsContent from "./TokenomicsContent";

export const metadata = {
  title: "Tokenomics - DChat",
  description:
    "DCHAT token economics, distribution, staking rewards, and governance mechanics.",
};

export default function TokenomicsPage() {
  return (
    <>
      <Header />
      <main className="pt-32 pb-20">
        <TokenomicsContent />
      </main>
      <Footer />
    </>
  );
}
