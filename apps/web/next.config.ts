import type { NextConfig } from 'next';

const config: NextConfig = {
  reactStrictMode: true,
  devIndicators: false,
  transpilePackages: [
    '@xterm/xterm',
    '@xterm/addon-fit',
    '@xterm/addon-webgl',
    '@xterm/addon-attach',
  ],
  async rewrites() {
    return [
      { source: '/api/:path*', destination: 'http://localhost:8000/api/:path*' },
      { source: '/ws/:path*', destination: 'http://localhost:8000/ws/:path*' },
      { source: '/events', destination: 'http://localhost:8000/events' },
      { source: '/health', destination: 'http://localhost:8000/health' },
    ];
  },
};

export default config;
