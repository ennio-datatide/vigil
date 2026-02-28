import type { MetadataRoute } from 'next';

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: 'Praefectus',
    short_name: 'Praefectus',
    description: 'AI Agent Orchestration Dashboard',
    start_url: '/dashboard',
    display: 'standalone',
    theme_color: '#7c5ef6',
    background_color: '#0a0b10',
    icons: [
      {
        src: '/icon.svg',
        sizes: 'any',
        type: 'image/svg+xml',
      },
    ],
  };
}
