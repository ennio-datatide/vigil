export function Tooltip({
  text,
  children,
  position = 'top',
}: {
  text: string;
  children: React.ReactNode;
  position?: 'top' | 'bottom';
}) {
  return (
    <span className="group relative inline-flex">
      {children}
      <span
        className={`pointer-events-none absolute left-1/2 z-50 -translate-x-1/2 whitespace-nowrap rounded-lg glass px-3 py-1.5 text-xs text-text opacity-0 shadow-lg transition-opacity duration-150 group-hover:opacity-100 ${
          position === 'top' ? 'bottom-full mb-2' : 'top-full mt-2'
        }`}
      >
        {text}
      </span>
    </span>
  );
}
