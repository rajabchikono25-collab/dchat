import Header from "@/components/Header";
import Footer from "@/components/Footer";
import BlockchainContent from "./BlockchainContent";

export const metadata = {
  title: "Blockchain Architecture - DChat",
  description:
    "Deep dive into DChat's tri-chain architecture, consensus mechanisms, and blockchain infrastructure.",
};

export default function BlockchainPage() {
  return (
    <>
      <Header />
      <main className="pt-32 pb-20">
        <BlockchainContent />
      </main>
      <Footer />
    </>
  );
}
