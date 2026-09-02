import { describe, expect, test } from "bun:test";
import {
  PWA_INSTALL_SNOOZE_MS,
  clearPwaInstallSnooze,
  isIosDevice,
  isStandaloneDisplayMode,
  readPwaInstallSnooze,
  requestNativePwaInstall,
  resolvePwaInstallAvailability,
  writePwaInstallSnooze,
  type BeforeInstallPromptEventLike,
} from "./pwaInstall";
import { canShowAutomaticPwaInstall } from "./PwaInstallPrompt";

function makeStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  };
}

describe("PWA install platform detection", () => {
  test("recognizes iPhone and iPadOS desktop user agents", () => {
    expect(isIosDevice({ userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)" })).toBe(true);
    expect(isIosDevice({ userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 5 })).toBe(true);
    expect(isIosDevice({ userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 0 })).toBe(false);
  });

  test("recognizes standalone display mode and legacy iOS standalone", () => {
    expect(isStandaloneDisplayMode({ matchMedia: () => ({ matches: true }) })).toBe(true);
    expect(isStandaloneDisplayMode({ standalone: true })).toBe(true);
    expect(isStandaloneDisplayMode({ matchMedia: () => ({ matches: false }) })).toBe(false);
  });

  test("prioritizes installed, native, then iOS guide states", () => {
    expect(resolvePwaInstallAvailability({ isInstalled: true, hasNativePrompt: true, isIos: true })).toBe("unavailable");
    expect(resolvePwaInstallAvailability({ isInstalled: false, hasNativePrompt: true, isIos: true })).toBe("native");
    expect(resolvePwaInstallAvailability({ isInstalled: false, hasNativePrompt: false, isIos: true })).toBe("ios-guide");
    expect(resolvePwaInstallAvailability({ isInstalled: false, hasNativePrompt: false, isIos: false })).toBe("unavailable");
  });
});

describe("PWA install lifecycle helpers", () => {
  test("gates automatic recommendations by route, demo, dialog, update, and snooze state", () => {
    const base = {
      routeSection: "fleet",
      demoMode: false,
      updatePromptVisible: false,
      dialogOpen: false,
      availability: "native" as const,
      isInstalled: false,
      automaticSnoozed: false,
      sessionHidden: false,
    };
    expect(canShowAutomaticPwaInstall(base)).toBe(true);
    expect(canShowAutomaticPwaInstall({ ...base, routeSection: "firmware" })).toBe(false);
    expect(canShowAutomaticPwaInstall({ ...base, demoMode: true })).toBe(false);
    expect(canShowAutomaticPwaInstall({ ...base, updatePromptVisible: true })).toBe(false);
    expect(canShowAutomaticPwaInstall({ ...base, dialogOpen: true })).toBe(false);
    expect(canShowAutomaticPwaInstall({ ...base, automaticSnoozed: true })).toBe(false);
    expect(canShowAutomaticPwaInstall({ ...base, sessionHidden: true })).toBe(false);
  });

  test("only invokes the native prompt when the request helper is called", async () => {
    let promptCalls = 0;
    const event = {
      preventDefault: () => undefined,
      prompt: async () => {
        promptCalls += 1;
      },
      userChoice: Promise.resolve({ outcome: "accepted" as const }),
    } as unknown as BeforeInstallPromptEventLike;

    expect(promptCalls).toBe(0);
    await expect(requestNativePwaInstall(event)).resolves.toBe("accepted");
    expect(promptCalls).toBe(1);
  });

  test("stores and expires a 30-day automatic recommendation snooze", () => {
    const storage = makeStorage();
    const now = 1_700_000_000_000;
    const until = writePwaInstallSnooze(storage, now);
    expect(until).toBe(now + PWA_INSTALL_SNOOZE_MS);
    expect(readPwaInstallSnooze(storage, now + 1)).toBe(until);
    expect(readPwaInstallSnooze(storage, until)).toBeNull();
    expect(storage.getItem("mains-aegis-web.pwa-install-snooze.v1")).toBeNull();
  });

  test("clears snooze best-effort", () => {
    const storage = makeStorage();
    writePwaInstallSnooze(storage, 1_700_000_000_000);
    clearPwaInstallSnooze(storage);
    expect(readPwaInstallSnooze(storage, 1_700_000_000_001)).toBeNull();
  });
});
