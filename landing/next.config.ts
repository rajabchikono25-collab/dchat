import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Use standalone output only for Docker deployments, not Vercel
  // output: "standalone",
};

export default nextConfig;
