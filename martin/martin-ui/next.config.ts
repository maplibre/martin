import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  images: {
    unoptimized: true,
  },
  output: "export",
  typescript: {
    ignoreBuildErrors: false,
  },
};

export default nextConfig;
