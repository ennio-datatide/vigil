'use client';

interface PraefectusLogoProps {
  collapsed?: boolean;
  size?: number;
  className?: string;
}

function LogoMark({ size = 24, className }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      className={className}
      aria-hidden="true"
    >
      <line x1="7" y1="12" x2="17" y2="6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <line x1="7" y1="12" x2="17" y2="12" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <line x1="17" y1="6" x2="17" y2="12" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <circle cx="7" cy="12" r="2.5" fill="currentColor" />
      <circle cx="17" cy="6" r="2.5" fill="currentColor" />
      <circle cx="17" cy="12" r="2.5" fill="currentColor" />
    </svg>
  );
}

export function PraefectusLogo({ collapsed = false, size = 24, className = '' }: PraefectusLogoProps) {
  return (
    <div className={`flex items-center gap-2 text-accent ${className}`}>
      <LogoMark size={size} />
      {!collapsed && (
        <span className="text-lg font-semibold tracking-tight text-text">
          Praefectus
        </span>
      )}
    </div>
  );
}
