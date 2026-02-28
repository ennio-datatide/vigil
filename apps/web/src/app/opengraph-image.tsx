import { ImageResponse } from 'next/og';

export const alt = 'Praefectus — AI Agent Orchestration Dashboard';
export const size = { width: 1200, height: 630 };
export const contentType = 'image/png';

export default function OGImage() {
  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          background: 'linear-gradient(135deg, #0a0b10 0%, #12132a 50%, #0a0b10 100%)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 24 }}>
          <svg
            width="80"
            height="80"
            viewBox="0 0 24 24"
            fill="none"
          >
            <line x1="7" y1="12" x2="17" y2="6" stroke="#7c5ef6" strokeWidth="1.5" strokeLinecap="round" />
            <line x1="7" y1="12" x2="17" y2="12" stroke="#7c5ef6" strokeWidth="1.5" strokeLinecap="round" />
            <line x1="17" y1="6" x2="17" y2="12" stroke="#7c5ef6" strokeWidth="1.5" strokeLinecap="round" />
            <circle cx="7" cy="12" r="2.5" fill="#7c5ef6" />
            <circle cx="17" cy="6" r="2.5" fill="#7c5ef6" />
            <circle cx="17" cy="12" r="2.5" fill="#7c5ef6" />
          </svg>
          <span
            style={{
              fontSize: 64,
              fontWeight: 700,
              color: '#e8e6f0',
              letterSpacing: '-0.02em',
            }}
          >
            Praefectus
          </span>
        </div>

        <span
          style={{
            fontSize: 28,
            color: '#9f9bae',
            marginTop: 16,
          }}
        >
          AI Agent Orchestration Dashboard
        </span>

        <div
          style={{
            width: 120,
            height: 4,
            borderRadius: 2,
            background: '#7c5ef6',
            marginTop: 24,
          }}
        />
      </div>
    ),
    { ...size },
  );
}
