import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { createPwaManifest, createPwaOptions } from "../../vite.config";
import { shouldShowPwaUpdatePrompt } from "./PwaUpdatePrompt";
import { resolveServiceWorkerTarget } from "./serviceWorkerTarget";

function runPublicFallbackRedirect(input: string): string | null {
  const html = readFileSync(new URL("../../public/404.html", import.meta.url), "utf8");
  const script = /<script>([\s\S]*?)<\/script>/.exec(html)?.[1];
  if (!script) throw new Error("public fallback script is missing");
  const url = new URL(input);
  let replacedUrl: string | null = null;
  const window = {
    location: {
      pathname: url.pathname,
      search: url.search,
      hash: url.hash,
      replace(value: string) {
        replacedUrl = new URL(value, url).href;
      },
    },
  };
  runInNewContext(script, { URLSearchParams, window });
  return replacedUrl;
}

describe("PWA manifest", () => {
  test("uses relative URLs for browser-static Pages builds", () => {
    const manifest = createPwaManifest("./");
    expect(manifest.start_url).toBe("./");
    expect(manifest.scope).toBe("./");
    expect(manifest.display).toBe("standalone");
    expect(manifest.icons).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          src: "./pwa/mains-aegis-icon-192.png",
          sizes: "192x192",
          purpose: "any",
        }),
        expect.objectContaining({
          src: "./pwa/mains-aegis-icon-512.png",
          sizes: "512x512",
          purpose: "any",
        }),
        expect.objectContaining({
          src: "./pwa/mains-aegis-icon-maskable-512.png",
          sizes: "512x512",
          purpose: "maskable",
        }),
      ]),
    );
  });

  test("keeps explicit deployment subpaths installable", () => {
    const manifest = createPwaManifest("/mains-aegis/");
    expect(manifest.start_url).toBe("/mains-aegis/");
    expect(manifest.scope).toBe("/mains-aegis/");
    expect(manifest.icons?.[0]?.src).toBe("/mains-aegis/pwa/mains-aegis-icon-192.png");
  });
});

describe("PWA workbox contract", () => {
  test("precache includes bundled static assets and excludes runtime APIs", () => {
    const options = createPwaOptions("./");
    expect(options.registerType).toBe("prompt");
    expect(options.injectRegister).toBe(false);
    expect(options.workbox?.globPatterns).toContain("**/*");
    expect(options.workbox?.importScripts).toContain("pwa-navigation-fallback.js");
    expect(options.includeAssets).toEqual(
      expect.arrayContaining([
        "favicon.svg",
        "favicon-dark.svg",
        "pwa/mains-aegis-icon-192.png",
        "pwa/mains-aegis-icon-512.png",
        "pwa/mains-aegis-icon-maskable-512.png",
        "pwa/mains-aegis-icon-dark-192.png",
        "pwa/mains-aegis-icon-dark-512.png",
        "pwa/mains-aegis-icon-dark-maskable-512.png",
      ]),
    );
    expect(options.workbox?.navigateFallback).toBe("index.html");
    expect(options.workbox?.maximumFileSizeToCacheInBytes).toBeGreaterThan(7 * 1024 * 1024);
    expect(options.workbox?.navigateFallbackDenylist).toHaveLength(3);
    expect(options.workbox?.navigateFallbackDenylist?.some((pattern) => pattern.test("/api/status"))).toBe(true);
    expect(options.workbox?.navigateFallbackDenylist?.some((pattern) => pattern.test("/events"))).toBe(true);
    expect(options.workbox?.navigateFallbackDenylist?.some((pattern) => pattern.test("/mains-aegis/docs/"))).toBe(true);
    expect(options.workbox?.navigateFallbackDenylist?.some((pattern) => pattern.test("/mains-aegis/connect"))).toBe(false);
  });

  test("keeps absolute deployment bases on the app shell fallback", () => {
    const options = createPwaOptions("/mains-aegis/");
    expect(options.workbox?.navigateFallback).toBe("index.html");
  });
});

describe("PWA service worker registration target", () => {
  test("roots relative Pages builds at the deployed app root on deep routes", () => {
    expect(
      resolveServiceWorkerTarget("./", "/mains-aegis/devices/demo"),
    ).toEqual({
      scriptUrl: "/mains-aegis/sw.js",
      scope: "/mains-aegis/",
    });
    expect(resolveServiceWorkerTarget("./", "/devices/demo")).toEqual({
      scriptUrl: "/sw.js",
      scope: "/",
    });
  });

  test("keeps explicit deployment subpaths stable", () => {
    expect(
      resolveServiceWorkerTarget("/mains-aegis/", "/mains-aegis/devices/demo"),
    ).toEqual({
      scriptUrl: "/mains-aegis/sw.js",
      scope: "/mains-aegis/",
    });
  });
});

describe("public static fallback", () => {
  test("preserves the deployment root for subpath direct routes", () => {
    expect(
      runPublicFallbackRedirect("https://example.test/mains-aegis/devices/demo?demo=true#power"),
    ).toBe(
      "https://example.test/mains-aegis/index.html?demo=true&spa_path=%2Fdevices%2Fdemo&spa_search=demo%3Dtrue&spa_hash=power",
    );
  });

  test("preserves the deployment root for bundled docs direct routes", () => {
    expect(
      runPublicFallbackRedirect("https://example.test/mains-aegis/docs/design/system-overview"),
    ).toBe(
      "https://example.test/mains-aegis/index.html?spa_path=%2Fdocs%2Fdesign%2Fsystem-overview",
    );
  });

  test("keeps root deployments at the domain root", () => {
    expect(runPublicFallbackRedirect("https://example.test/connect?demo=true")).toBe(
      "https://example.test/index.html?demo=true&spa_path=%2Fconnect&spa_search=demo%3Dtrue",
    );
  });
});

describe("PWA update prompt visibility", () => {
  test("shows only owner-facing lifecycle states", () => {
    expect(shouldShowPwaUpdatePrompt("idle")).toBe(false);
    expect(shouldShowPwaUpdatePrompt("updated")).toBe(false);
    expect(shouldShowPwaUpdatePrompt("ready")).toBe(true);
    expect(shouldShowPwaUpdatePrompt("activating")).toBe(true);
    expect(shouldShowPwaUpdatePrompt("offlineReady")).toBe(true);
    expect(shouldShowPwaUpdatePrompt("error")).toBe(true);
  });
});
