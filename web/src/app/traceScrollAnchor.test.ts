import { describe, expect, test } from "bun:test";

import { captureTraceScrollAnchor, resolveAnchoredTraceScrollTop } from "./traceScrollAnchor";

const entries = Array.from({ length: 8 }, (_, index) => ({ id: `log-${index}` }));
const offsets = entries.map((_, index) => index * 50);

describe("trace scroll anchoring", () => {
  test("captures the first visible row and its intra-row offset", () => {
    expect(captureTraceScrollAnchor(entries, offsets, 165)).toEqual({ id: "log-3", offset: 15 });
  });

  test("keeps the same row fixed when older buffered rows are trimmed", () => {
    const anchor = captureTraceScrollAnchor(entries, offsets, 165);
    const trimmedEntries = entries.slice(2);
    const trimmedOffsets = trimmedEntries.map((_, index) => index * 50);

    expect(
      resolveAnchoredTraceScrollTop({
        anchor,
        entries: trimmedEntries,
        offsets: trimmedOffsets,
        currentScrollTop: 165,
        maxScrollTop: 500,
        pinnedToBottom: false,
      }),
    ).toBe(65);
  });

  test("falls back to the nearest valid scroll top when the anchor row is gone", () => {
    const anchor = { id: "log-1", offset: 20 };
    const trimmedEntries = entries.slice(3);
    const trimmedOffsets = trimmedEntries.map((_, index) => index * 50);

    expect(
      resolveAnchoredTraceScrollTop({
        anchor,
        entries: trimmedEntries,
        offsets: trimmedOffsets,
        currentScrollTop: 300,
        maxScrollTop: 180,
        pinnedToBottom: false,
      }),
    ).toBe(180);
  });

  test("stays pinned to the newest records at the bottom", () => {
    expect(
      resolveAnchoredTraceScrollTop({
        anchor: { id: "log-3", offset: 15 },
        entries,
        offsets,
        currentScrollTop: 165,
        maxScrollTop: 900,
        pinnedToBottom: true,
      }),
    ).toBe(900);
  });
});
