import type { Metadata } from 'next';
import { Providers } from './providers';
import '@/styles/globals.css';

export const metadata: Metadata = {
  metadataBase: new URL(process.env.NEXT_PUBLIC_BASE_URL ?? 'http://localhost:3000'),
  title: {
    default: 'Vigil',
    template: '%s — Vigil',
  },
  description: 'AI Agent Orchestration Dashboard',
  openGraph: {
    title: 'Vigil',
    description: 'AI Agent Orchestration Dashboard',
    siteName: 'Vigil',
    type: 'website',
  },
  twitter: {
    card: 'summary_large_image',
    title: 'Vigil',
    description: 'AI Agent Orchestration Dashboard',
  },
  manifest: '/manifest.webmanifest',
  other: {
    'theme-color': '#7c5ef6',
  },
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className="dark">
      <body className="min-h-screen bg-bg text-text antialiased nebula-bg">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
