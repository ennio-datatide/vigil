import { ImageResponse } from 'next/og';

export const size = { width: 180, height: 180 };
export const contentType = 'image/png';

export default function AppleIcon() {
  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          background: '#0a0b10',
        }}
      >
        <svg
          width="120"
          height="120"
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
      </div>
    ),
    { ...size },
  );
}
