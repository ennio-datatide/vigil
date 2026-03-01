'use client';

import { AnimatePresence, motion } from 'framer-motion';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useState } from 'react';
import { NewSession } from '@/components/dashboard/new-session';
import { NotificationBell } from '@/components/dashboard/notification-bell';
import { PraefectusLogo } from '@/components/ui/praefectus-logo';
import { ToastContainer } from '@/components/ui/toast';

const NAV_ITEMS = [
  {
    href: '/dashboard',
    label: 'Dashboard',
    icon: (
      <svg
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <rect x="3" y="3" width="7" height="7" rx="1" />
        <rect x="14" y="3" width="7" height="7" rx="1" />
        <rect x="3" y="14" width="7" height="7" rx="1" />
        <rect x="14" y="14" width="7" height="7" rx="1" />
      </svg>
    ),
    match: 'exact' as const,
  },
  {
    href: '/dashboard/history',
    label: 'History',
    icon: (
      <svg
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <circle cx="12" cy="12" r="10" />
        <polyline points="12 6 12 12 16 14" />
      </svg>
    ),
    match: 'prefix' as const,
  },
  {
    href: '/dashboard/projects',
    label: 'Projects',
    icon: (
      <svg
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
      </svg>
    ),
    match: 'prefix' as const,
  },
  {
    href: '/dashboard/pipelines',
    label: 'Pipelines',
    icon: (
      <svg
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" />
      </svg>
    ),
    match: 'prefix' as const,
  },
  {
    href: '/dashboard/settings',
    label: 'Settings',
    icon: (
      <svg
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.32 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
    ),
    match: 'prefix' as const,
  },
];

function isActive(pathname: string, href: string, match: 'exact' | 'prefix') {
  if (match === 'exact') return pathname === href;
  return pathname.startsWith(href);
}

function isSessionPage(pathname: string) {
  return /^\/dashboard\/sessions\/.+/.test(pathname);
}

export default function DashboardLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const [collapsed, setCollapsed] = useState(false);
  const immersive = isSessionPage(pathname);

  return (
    <div className="flex h-screen flex-col md:flex-row">
      {/* Mobile top bar */}
      {!immersive && (
        <header className="flex shrink-0 items-center justify-between border-b border-border-subtle px-4 py-3 md:hidden">
          <PraefectusLogo />
          <NotificationBell />
        </header>
      )}

      {/* Desktop sidebar (collapsible) */}
      <aside
        className={`hidden shrink-0 border-r border-border-subtle glass-strong md:flex md:flex-col transition-all duration-200 ${
          collapsed ? 'w-14' : 'w-56'
        }`}
      >
        <div className={`flex items-center justify-between p-4 ${collapsed ? 'px-2' : ''}`}>
          <PraefectusLogo collapsed={collapsed} />
          {!collapsed && <NotificationBell />}
        </div>

        <nav className={`flex-1 space-y-1 ${collapsed ? 'px-1' : 'px-3'}`}>
          {NAV_ITEMS.map((item) => {
            const active = isActive(pathname, item.href, item.match);
            return (
              <Link
                key={item.href}
                href={item.href}
                title={collapsed ? item.label : undefined}
                className={`group relative flex items-center gap-3 rounded-lg px-3 py-2 text-[13px] transition-colors ${
                  collapsed ? 'justify-center px-2' : ''
                } ${
                  active
                    ? 'bg-accent-glow text-text'
                    : 'text-text-muted hover:bg-surface-hover hover:text-text'
                }`}
              >
                <span className={active ? 'text-accent' : 'text-text-muted group-hover:text-text'}>
                  {item.icon}
                </span>
                {!collapsed && <span>{item.label}</span>}
              </Link>
            );
          })}
        </nav>

        <button
          type="button"
          onClick={() => setCollapsed((c) => !c)}
          className="mx-3 mb-3 flex items-center justify-center rounded-lg py-2 text-text-muted hover:bg-surface-hover hover:text-text transition-colors"
          aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            className={`transition-transform ${collapsed ? 'rotate-180' : ''}`}
          >
            <polyline points="11 17 6 12 11 7" />
            <polyline points="18 17 13 12 18 7" />
          </svg>
        </button>
      </aside>

      {/* Main content */}
      <main className="min-h-0 flex-1 overflow-auto">
        <AnimatePresence mode="wait">
          <motion.div
            key={pathname}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.2, ease: [0.25, 0.1, 0.25, 1] }}
            className="h-full"
          >
            {children}
          </motion.div>
        </AnimatePresence>
      </main>

      {/* Mobile bottom tab bar */}
      {!immersive && (
        <nav className="glass-strong shrink-0 border-t border-border-subtle md:hidden">
          <div className="flex items-center justify-around">
            {NAV_ITEMS.map((item) => {
              const active = isActive(pathname, item.href, item.match);
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  className={`relative flex min-h-[56px] flex-1 flex-col items-center justify-center gap-0.5 text-[10px] transition-colors ${
                    active ? 'text-accent' : 'text-text-muted'
                  }`}
                >
                  {active && (
                    <motion.span
                      layoutId="tab-indicator"
                      className="absolute top-0 h-0.5 w-8 rounded-full bg-accent"
                      transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                    />
                  )}
                  <span className={`transition-transform ${active ? 'scale-110' : ''}`}>
                    {item.icon}
                  </span>
                  <span className="font-medium">{item.label}</span>
                </Link>
              );
            })}
          </div>
        </nav>
      )}

      <NewSession />
      <ToastContainer />
    </div>
  );
}
