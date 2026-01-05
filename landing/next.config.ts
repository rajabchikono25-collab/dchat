import type { NextConfig } from "next";
import path from "path";

const nextConfig: NextConfig = {
  // Use standalone output only for Docker deployments, not Vercel
  // output: "standalone",

  // Monorepo: avoid workspace root inference warnings
  outputFileTracingRoot: path.join(__dirname, ".."),
};

export default nextConfig;
