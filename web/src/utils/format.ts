export function formatVoltage(mv: number | null | undefined): string {
  if (mv === null || mv === undefined) return "--";
  return `${(mv / 1000).toFixed(2)} V`;
}

export function formatCurrent(ma: number | null | undefined): string {
  if (ma === null || ma === undefined) return "--";
  return `${ma} mA`;
}

export function formatTemp(c: number | null | undefined): string {
  if (c === null || c === undefined) return "--";
  return `${c} C`;
}

export function formatPercent(value: number | null | undefined): string {
  if (value === null || value === undefined) return "--";
  return `${value}%`;
}

export function timeAgo(iso: string | null): string {
  if (!iso) return "never";
  const delta = Date.now() - new Date(iso).getTime();
  if (Number.isNaN(delta)) return "unknown";
  const seconds = Math.max(0, Math.floor(delta / 1000));
  if (seconds < 5) return "now";
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ago`;
}
