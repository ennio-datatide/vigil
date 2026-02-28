export function SessionCardSkeleton() {
  return (
    <div className="glass animate-pulse rounded-xl p-4">
      <div className="flex items-center justify-between">
        <div className="h-3 w-16 rounded-md bg-surface-hover" />
        <div className="h-5 w-20 rounded-full bg-surface-hover" />
      </div>
      <div className="mt-3 space-y-2">
        <div className="h-3 w-full rounded-md bg-surface-hover" />
        <div className="h-3 w-3/4 rounded-md bg-surface-hover" />
      </div>
      <div className="mt-4 flex items-center justify-between">
        <div className="h-3 w-20 rounded-md bg-surface-hover" />
        <div className="h-3 w-10 rounded-md bg-surface-hover" />
      </div>
    </div>
  );
}
