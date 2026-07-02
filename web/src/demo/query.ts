import type { DemoSeed } from "../fixtures/mockDevices";

export const DEMO_QUERY_PARAM = "demo";
export const DEFAULT_DEMO_SEED: DemoSeed = "default";

export function isDemoQueryEnabled(search = currentSearch()): boolean {
  const value = new URLSearchParams(search).get(DEMO_QUERY_PARAM)?.trim().toLowerCase();
  return value === "true";
}

export function demoQuerySeed(search = currentSearch()): DemoSeed | null {
  if (!isDemoQueryEnabled(search)) return null;
  return DEFAULT_DEMO_SEED;
}

function currentSearch(): string {
  return typeof window === "undefined" ? "" : window.location.search;
}
