export type TraceAnchorEntry = { id: string };

export type TraceScrollAnchor = {
  id: string;
  offset: number;
};

export function captureTraceScrollAnchor(
  entries: TraceAnchorEntry[],
  offsets: number[],
  scrollTop: number,
): TraceScrollAnchor | null {
  if (entries.length === 0) return null;

  let low = 0;
  let high = entries.length - 1;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if ((offsets[mid] ?? 0) <= scrollTop) {
      low = mid;
    } else {
      high = mid - 1;
    }
  }

  return {
    id: entries[low].id,
    offset: scrollTop - (offsets[low] ?? 0),
  };
}

export function resolveAnchoredTraceScrollTop({
  anchor,
  entries,
  offsets,
  currentScrollTop,
  maxScrollTop,
  pinnedToBottom,
}: {
  anchor: TraceScrollAnchor | null;
  entries: TraceAnchorEntry[];
  offsets: number[];
  currentScrollTop: number;
  maxScrollTop: number;
  pinnedToBottom: boolean;
}) {
  if (pinnedToBottom) return maxScrollTop;

  const anchorIndex = anchor ? entries.findIndex((entry) => entry.id === anchor.id) : -1;
  if (anchorIndex < 0) return Math.min(currentScrollTop, maxScrollTop);

  const anchorOffset = anchor?.offset ?? 0;
  return Math.min(Math.max(0, (offsets[anchorIndex] ?? 0) + anchorOffset), maxScrollTop);
}
