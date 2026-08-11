import { describe, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { DeviceRecord } from "../api/types";
import {
  ACTIVE_ALERT_REFRESH_MS,
  ACTIVE_ALERT_REQUEST_TIMEOUT_MS,
  activeAlertSeverity,
  audibleAlertCount,
  deviceSettingsAvailable,
  normalizeBasePath,
  resolveManualHttpRememberedChannel,
  resolveBrandLogoAsset,
  resolveDemoTheme,
  resolveDevdTarget,
  resolveOwnerFacingDevdTarget,
  resolveStartupDevdTarget,
} from "./App";
import {
  resolveSpaFallbackInitialPath,
  restoreSpaFallbackHash,
  restoreSpaFallbackLocation,
} from "./spaFallback";

function makeRecord(overrides: Partial<DeviceRecord>): DeviceRecord {
  return {
    target: {
      deviceId: "mains-aegis-legacy-usb",
      baseUrl: "serial:mains-aegis-legacy-usb",
      alias: "Legacy USB UPS",
      location: "Bench",
      addedAt: "2026-06-07T00:00:00.000Z",
      transport: "serial",
      preferredTransport: "serial",
      rememberedChannels: {
        serial: {
          seenAt: "2026-06-07T00:00:00.000Z",
        },
      },
    },
    identity: null,
    network: null,
    settings: null,
    status: null,
    connectionState: "online",
    streamState: "streaming",
    error: null,
    lastUpdated: "2026-06-07T00:00:00.000Z",
    serial: {
      connected: true,
      source: "web_serial",
      protocol: "mains-aegis.cdc.v1",
      logs: [],
      trace: [],
    },
    ...overrides,
  };
}

describe("active alert presentation", () => {
  test("uses a short deterministic refresh cadence", () => {
    expect(ACTIVE_ALERT_REFRESH_MS).toBe(2_000);
    expect(ACTIVE_ALERT_REQUEST_TIMEOUT_MS).toBe(1_500);
  });

  test("keeps the highest active severity authoritative", () => {
    expect(activeAlertSeverity(null)).toBeNull();
    expect(activeAlertSeverity({ alerts: [] })).toBeNull();
    expect(
      activeAlertSeverity({
        alerts: [
          {
            alert_id: "mains_absent_dc",
            instance_id: 1,
            severity: "warning",
            sound_state: "audible",
          },
          {
            alert_id: "shutdown_protection",
            instance_id: 2,
            severity: "critical",
            sound_state: "muted",
          },
        ],
      }),
    ).toBe("critical");
  });

  test("reports audible alerts independently from active severity", () => {
    expect(
      audibleAlertCount({
        alerts: [
          {
            alert_id: "mains_absent_dc",
            instance_id: 1,
            severity: "warning",
            sound_state: "audible",
          },
          {
            alert_id: "high_stress",
            instance_id: 2,
            severity: "warning",
            sound_state: "system_silent",
          },
        ],
      }),
    ).toBe(1);
  });

  test("keeps global alerts inside the existing topbar metrics", () => {
    const source = readFileSync(join(import.meta.dir, "App.tsx"), "utf8");
    expect(source).not.toContain("PersistentFleetAlertStatus");
    expect(source).not.toContain("persistent-alert-banner");
    expect(source).toContain("activeAlertsByDevice={fleetAlerts.snapshots}");
    expect(source).toContain('const criticalTarget = alertTargetDeviceId("critical")');
    expect(source).toContain('const warningTarget = alertTargetDeviceId("warning")');
    expect(source).toContain("top-metric is-actionable");
    expect(source).toContain("currentRecords.map(async (record) =>");
  });

  test("clears single-flight refreshes using the tracked promise", () => {
    const source = readFileSync(join(import.meta.dir, "App.tsx"), "utf8");
    expect(source).toContain("const trackedRequest = request.finally(() => {");
    expect(source).not.toContain("refreshInFlight.current === request");
    expect(source).toContain("refreshGeneration.current += 1;");
    expect(source).toContain("refreshInFlight.current = null;");
    expect(source).toContain("[recordKey, refresh]");
  });
});

function withMockWindow<T>(
  value: typeof globalThis.window,
  callback: () => T,
): T {
  const originalWindow = globalThis.window;
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value,
  });
  try {
    return callback();
  } finally {
    if (originalWindow === undefined) {
      delete (globalThis as typeof globalThis & { window?: Window }).window;
    } else {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: originalWindow,
      });
    }
  }
}

describe("normalizeBasePath", () => {
  test("treats Vite relative base as site root", () => {
    expect(normalizeBasePath("./")).toBe("/");
    expect(normalizeBasePath(".")).toBe("/");
  });

  test("derives a project path for relative base deployments", () => {
    expect(normalizeBasePath("./", "/mains-aegis")).toBe("/");
    expect(normalizeBasePath("./", "/mains-aegis/")).toBe("/mains-aegis/");
    expect(normalizeBasePath("./", "/mains-aegis/?demo=true")).toBe(
      "/mains-aegis/",
    );
    expect(normalizeBasePath("./", "/mains-aegis/index.html")).toBe(
      "/mains-aegis/",
    );
    expect(normalizeBasePath("./", "/mains-aegis/404.html")).toBe(
      "/mains-aegis/",
    );
    expect(normalizeBasePath("./", "/product/mains-aegis/index.html")).toBe(
      "/product/mains-aegis/",
    );
    expect(normalizeBasePath("./", "/product/mains-aegis")).toBe(
      "/product/mains-aegis/",
    );
    expect(normalizeBasePath("./", "/product/mains-aegis/")).toBe(
      "/product/mains-aegis/",
    );
    expect(normalizeBasePath("./", "/product/mains-aegis/404.html")).toBe(
      "/product/mains-aegis/",
    );
    expect(normalizeBasePath("./", "/mains-aegis/docs/index.html")).toBe(
      "/mains-aegis/",
    );
    expect(
      normalizeBasePath("./", "/mains-aegis/docs/design/system-overview.html"),
    ).toBe("/mains-aegis/");
    expect(normalizeBasePath("./", "/mains-aegis/devices/demo")).toBe(
      "/mains-aegis/",
    );
    expect(normalizeBasePath("./", "/devices/demo")).toBe("/");
    expect(normalizeBasePath("./", "/connect")).toBe("/");
    expect(normalizeBasePath("./", "/mains-aegis/connect")).toBe(
      "/mains-aegis/",
    );
  });

  test("does not treat unknown single-segment routes as deployment roots", () => {
    expect(normalizeBasePath("./", "/foo")).toBe("/");
  });

  test("preserves trailing slash single-segment deployment roots", () => {
    expect(normalizeBasePath("./", "/mains-aegis/")).toBe("/mains-aegis/");
  });

  test("does not treat unknown deep routes as deployment roots", () => {
    expect(normalizeBasePath("./", "/mains-aegis/api/openapi.json")).toBe("/");
    expect(normalizeBasePath("./", "/product/mains-aegis/future/page")).toBe(
      "/",
    );
  });

  test("does not derive the app base from the runtime bundle URL", () => {
    expect(
      normalizeBasePath(
        "./",
        "/app/foo/bar",
        "https://mains-aegis.example/app/assets/index.js",
      ),
    ).toBe("/");
    expect(
      normalizeBasePath(
        "./",
        "/mains-aegis/devices/demo",
        "https://ivanli-cn.github.io/mains-aegis/assets/index.js",
      ),
    ).toBe("/mains-aegis/");
    expect(
      normalizeBasePath(
        "./",
        "/devices/demo",
        "https://cdn.example/assets/index.js",
      ),
    ).toBe("/");
  });

  test("keeps absolute deployment subpaths", () => {
    expect(normalizeBasePath("/mains-aegis")).toBe("/mains-aegis/");
    expect(normalizeBasePath("/mains-aegis/")).toBe("/mains-aegis/");
  });

  test("normalizes bare path overrides", () => {
    expect(normalizeBasePath("mains-aegis")).toBe("/mains-aegis/");
  });
});

describe("resolveBrandLogoAsset", () => {
  test("uses the light-theme mark by default", () => {
    expect(resolveBrandLogoAsset(new URLSearchParams(), true)).toBe(
      "mains-aegis-logo-mark-color-light.svg",
    );
  });

  test("does not expose review palette overrides", () => {
    const params = new URLSearchParams({ brand_logo: "08-emerald-slate" });
    expect(resolveBrandLogoAsset(params, true)).toBe(
      "mains-aegis-logo-mark-color-light.svg",
    );
  });

  test("uses the dark-theme default mark for a dark demo", () => {
    expect(resolveBrandLogoAsset(new URLSearchParams(), true, "dark")).toBe(
      "mains-aegis-logo-mark-color-dark.svg",
    );
  });
});

describe("resolveDemoTheme", () => {
  test("allows only explicit demo light and dark themes", () => {
    expect(resolveDemoTheme(new URLSearchParams({ theme: "dark" }), true)).toBe(
      "dark",
    );
    expect(resolveDemoTheme(new URLSearchParams({ theme: "light" }), true)).toBe(
      "light",
    );
    expect(resolveDemoTheme(new URLSearchParams({ theme: "system" }), true)).toBeNull();
  });

  test("does not expose demo theme overrides outside demo mode", () => {
    expect(resolveDemoTheme(new URLSearchParams({ theme: "dark" }), false)).toBeNull();
  });
});

describe("write-spa-fallback", () => {
  test("keeps explicit custom-domain subpath roots in the 404 bootstrap", () => {
    const dir = mkdtempSync(join(tmpdir(), "mains-aegis-404-"));
    const outFile = join(dir, "404.html");
    try {
      execFileSync(
        "node",
        [
          "tools/pages/write-spa-fallback.mjs",
          outFile,
          "mains-aegis",
          "/product/mains-aegis/",
        ],
        { cwd: join(import.meta.dir, "../../.."), stdio: "pipe" },
      );
      const html = readFileSync(outFile, "utf8");
      expect(html).toContain('var configuredRoot = "/product/mains-aegis/";');
      expect(html).toContain(
        'var configuredRootNoSlash = configuredRoot.replace(/\\/$/, "");',
      );
      expect(html).toContain(
        "if (configuredRoot && (path === configuredRootNoSlash || path.indexOf(configuredRoot) === 0))",
      );
      expect(html).toContain(
        'path = path === configuredRootNoSlash ? "/" : path.slice(configuredRoot.length - 1) || "/";',
      );
      expect(html).toContain(
        "if (!configuredRoot && (isGithubProjectHost || path.indexOf(repoPrefix) === 0))",
      );
      expect(html).toContain('if (path === "/docs" || path === "/docs/"');
      expect(html).toContain('docsPath += ".html";');
      expect(html).toContain('docsPath += "/index.html";');
      expect(html).toContain('docsPath = docsPathWithoutSlash + ".html";');
      expect(html).toContain(
        'docsPath = docsPathWithoutSlash + "/index.html";',
      );
      expect(html).toContain(
        "/^\\/docs\\/(handbook|design|manual)$/i.test(docsPath)",
      );
      expect(html).toContain("docsPath.replace(/\\/+$/, \"\")");
      expect(html).toContain("!/\\/[^/]*\\.[^/]+$/i.test(docsPath)");
      expect(html).toContain("} else {");
      expect(html).toContain('docsPath = "/docs/404.html";');
      expect(html).toContain("window.location.replace(appRoot +");
      expect(html).toContain('var docsPath = path === "/docs" || path === "/docs/"');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("relativize-docs-html", () => {
  test("rewrites docs root links to per-file relative links", () => {
    const dir = mkdtempSync(join(tmpdir(), "mains-aegis-docs-"));
    try {
      mkdirSync(join(dir, "design"), { recursive: true });
      mkdirSync(join(dir, "static/js"), { recursive: true });
      mkdirSync(join(dir, "static"), { recursive: true });
      writeFileSync(
        join(dir, "index.html"),
        '<a href="/">Home</a><a href="/docs/">Docs</a><script src="/static/js/app.js"></script>',
      );
      writeFileSync(
        join(dir, "design", "page.html"),
        '<a href="/">Home</a><a href="/design/">Design</a><img src="/brand/mark.svg">',
      );
      writeFileSync(
        join(dir, "static/js/app.js"),
        'var n={};n.p="/docs/";var site={base:"/",logo:"/brand/mark.svg",themeConfig:{nav:[{link:"/design/system-overview"}]},pages:[{path:"/docs/design/system-overview",routePath:"/docs/design/system-overview"}]};var search={link:`${item.domain}${(0,n.AP)(item.routePath)}`};var mdx={href:"/design/front-panel-ui-design",src:"/ui/self-check.png",external:"https://reactjs.org/docs/error-decoder.html"};',
      );
      writeFileSync(
        join(dir, "static/search_index.json"),
        '{"hits":[{"routePath":"/docs/design/system-overview","title":"System"}]}',
      );
      execFileSync("node", ["tools/pages/relativize-docs-html.mjs", dir, "/docs/"], {
        cwd: join(import.meta.dir, "../../.."),
        stdio: "pipe",
      });

      expect(readFileSync(join(dir, "index.html"), "utf8")).toContain(
        'href="./"',
      );
      const pageHtml = readFileSync(join(dir, "design", "page.html"), "utf8");
      expect(pageHtml).toContain('href="../"');
      expect(pageHtml).toContain('href="../design/"');
      expect(pageHtml).toContain('src="../brand/mark.svg"');
      const js = readFileSync(join(dir, "static/js/app.js"), "utf8");
      expect(js).toContain(
        'globalThis.__mainsAegisDocsBase=new URL("../../",document.currentScript&&document.currentScript.src||location.href);',
      );
      expect(js).toContain('n.p=globalThis.__mainsAegisDocsBase.href');
      expect(js).toContain(
        "base:globalThis.__mainsAegisDocsBase.pathname",
      );
      expect(js).toContain(
        'logo:new URL("brand/mark.svg",globalThis.__mainsAegisDocsBase).pathname',
      );
      expect(js).toContain(
        'link:new URL("design/system-overview",globalThis.__mainsAegisDocsBase).pathname',
      );
      expect(js).toContain(
        'path:new URL("design/system-overview",globalThis.__mainsAegisDocsBase).pathname',
      );
      expect(js).toContain('routePath:"/design/system-overview"');
      expect(js).toContain(
        'href:new URL("design/front-panel-ui-design",globalThis.__mainsAegisDocsBase).pathname',
      );
      expect(js).toContain(
        'src:new URL("ui/self-check.png",globalThis.__mainsAegisDocsBase).pathname',
      );
      expect(js).toContain(
        "link:`${item.domain}${(0,n.pJ)((0,n.AP)(item.routePath))}`",
      );
      expect(js).toContain(
        'external:"https://reactjs.org/docs/error-decoder.html"',
      );
      expect(readFileSync(join(dir, "static/search_index.json"), "utf8")).toContain(
        '"routePath":"/design/system-overview"',
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("does not fail when docs runtime public path shape changes", () => {
    const dir = mkdtempSync(join(tmpdir(), "mains-aegis-docs-"));
    try {
      mkdirSync(join(dir, "static/js"), { recursive: true });
      writeFileSync(join(dir, "index.html"), '<a href="/">Home</a>');
      writeFileSync(join(dir, "static/js/app.js"), "globalThis.__docsBase='/docs/';");

      execFileSync("node", ["tools/pages/relativize-docs-html.mjs", dir, "/docs/"], {
        cwd: join(import.meta.dir, "../../.."),
        stdio: "pipe",
      });

      expect(readFileSync(join(dir, "index.html"), "utf8")).toContain(
        'href="./"',
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("resolveSpaFallbackInitialPath", () => {
  test("restores a deep route from the Pages 404 bootstrap", () => {
    const params = new URLSearchParams({
      spa_path: "/devices/mains-aegis-01/firmware",
    });

    expect(resolveSpaFallbackInitialPath(params)).toBe(
      "/devices/mains-aegis-01/firmware",
    );
  });

  test("keeps hash fragments out of the routed pathname", () => {
    const params = new URLSearchParams({
      spa_path: "/docs/design/system-overview",
      spa_hash: "power",
    });

    expect(resolveSpaFallbackInitialPath(params)).toBe(
      "/docs/design/system-overview",
    );
  });

  test("normalizes docs HTML fallback paths to SPA routes", () => {
    const params = new URLSearchParams({
      spa_path: "/docs/design/system-overview.html",
    });

    expect(resolveSpaFallbackInitialPath(params)).toBe(
      "/docs/design/system-overview",
    );
  });

  test("ignores invalid paths", () => {
    expect(
      resolveSpaFallbackInitialPath(new URLSearchParams({ spa_path: "assets" })),
    ).toBeUndefined();
  });
});

describe("restoreSpaFallbackLocation", () => {
  test("restores original search params and hash fragments on the current URL", () => {
    let replacedUrl = "";
    withMockWindow(
      {
        location: new URL(
          "https://mains-aegis.example/?spa_path=/devices/demo&spa_search=demo%3Dtrue%26devd_target%3Dsame-origin&spa_hash=power",
        ),
        history: {
          state: null,
          replaceState: (_state: unknown, _title: string, url: URL) => {
            replacedUrl = url.href;
          },
        },
      },
      () =>
        restoreSpaFallbackLocation(
          new URLSearchParams({
            spa_path: "/devices/demo",
            spa_search: "demo=true&devd_target=same-origin",
            spa_hash: "power",
          }),
        ),
    );

    expect(replacedUrl).toBe(
      "https://mains-aegis.example/devices/demo?demo=true&devd_target=same-origin#power",
    );
  });

  test("preserves the deployment root when restoring a project-page fallback", () => {
    let replacedUrl = "";
    withMockWindow(
      {
        location: new URL(
          "https://ivanli-cn.github.io/mains-aegis/?spa_path=/devices/demo",
        ),
        history: {
          state: null,
          replaceState: (_state: unknown, _title: string, url: URL) => {
            replacedUrl = url.href;
          },
        },
      },
      () =>
        restoreSpaFallbackLocation(
          new URLSearchParams({ spa_path: "/devices/demo" }),
        ),
    );

    expect(replacedUrl).toBe(
      "https://ivanli-cn.github.io/mains-aegis/devices/demo",
    );
  });

  test("removes fallback transport params when restoring query-only URLs", () => {
    let replacedUrl = "";
    withMockWindow(
      {
        location: new URL(
          "https://mains-aegis.example/?spa_path=/devices/demo&spa_search=demo%3Dtrue",
        ),
        history: {
          state: null,
          replaceState: (_state: unknown, _title: string, url: URL) => {
            replacedUrl = url.href;
          },
        },
      },
      () =>
        restoreSpaFallbackLocation(
          new URLSearchParams({
            spa_path: "/devices/demo",
            spa_search: "demo=true",
          }),
        ),
    );

    expect(replacedUrl).toBe("https://mains-aegis.example/devices/demo?demo=true");
  });

  test("restores deep-link path when no search or hash was forwarded", () => {
    let replacedUrl = "";
    withMockWindow(
      {
        location: new URL("https://mains-aegis.example/?spa_path=/docs"),
        history: {
          state: null,
          replaceState: (_state: unknown, _title: string, url: URL) => {
            replacedUrl = url.href;
          },
        },
      },
      () => restoreSpaFallbackHash(new URLSearchParams({ spa_path: "/docs" })),
    );

    expect(replacedUrl).toBe("https://mains-aegis.example/docs");
  });

  test("clears only fallback query params for hash-only restored URLs", () => {
    let replacedUrl = "";
    withMockWindow(
      {
        location: new URL(
          "https://mains-aegis.example/?demo=true&spa_path=/docs/design/system-overview&spa_hash=power",
        ),
        history: {
          state: null,
          replaceState: (_state: unknown, _title: string, url: URL) => {
            replacedUrl = url.href;
          },
        },
      },
      () =>
        restoreSpaFallbackLocation(
          new URLSearchParams({
            spa_path: "/docs/design/system-overview",
            spa_hash: "power",
          }),
        ),
    );

    expect(replacedUrl).toBe(
      "https://mains-aegis.example/docs/design/system-overview?demo=true#power",
    );
  });
});

describe("restoreSpaFallbackHash", () => {
  test("keeps the legacy helper as an alias for location restoration", () => {
    let replacedUrl = "";
    withMockWindow(
      {
        location: new URL(
          "https://mains-aegis.example/?spa_path=/docs&spa_hash=power",
        ),
        history: {
          state: null,
          replaceState: (_state: unknown, _title: string, url: URL) => {
            replacedUrl = url.href;
          },
        },
      },
      () =>
        restoreSpaFallbackHash(
          new URLSearchParams({ spa_path: "/docs", spa_hash: "power" }),
        ),
    );

    expect(replacedUrl).toBe("https://mains-aegis.example/docs#power");
  });
});

describe("deviceSettingsAvailable", () => {
  test("returns false for USB records without real settings support", () => {
    expect(deviceSettingsAvailable(makeRecord({ settings: null }))).toBe(false);
  });

  test("returns true for USB records with real settings", () => {
    expect(
      deviceSettingsAvailable(
        makeRecord({
          settings: {
            wifi: {
              configured: false,
              ssid: null,
            },
            log_level: "info",
            manual_charge: {
              target: "full_100",
              speed: "ma_500",
              timer_h: 2,
              power_path: "auto",
            },
            charge_capabilities: {
              target_voltage_mv: 16800,
              normal_current_ma: 500,
              dc_derated_current_ma: 100,
              dcin_input_limit_ma: 1000,
              max_output_current_ma: 3500,
              usb_pd_high_power_min_voltage_mv: 9000,
              usb_pd_high_power_max_voltage_mv: 20000,
              usb_pd_high_power_min_power_mw: 20000,
              loop_start_max_power_without_confirm_w10: 20,
              loop_stop_power_latched_w10: 30,
              loop_telemetry_miss_limit: 2,
              supported_power_paths: ["auto", "dcin", "usbc"],
              auto_path_priority: ["usbc_pd_high_power", "dcin", "usbc"],
            },
            advanced_power: {
              standby_drop_mv: 700,
              input_uvlo_cutoff_mv: 11300,
              input_uvlo_recover_mv: 11500,
              input_uvlo_required_samples: 3,
              source_limited_enter_delta_ma: 2500,
            },
            advanced_power_capabilities: {
              rated_vout_mv: 12000,
              standby_drop_mv: { default: 700, min: 0, max: 3000, step: 20 },
              input_uvlo_cutoff_mv: { default: 11300, min: 5000, max: 20000, step: 20 },
              input_uvlo_recover_mv: { default: 11500, min: 5000, max: 20000, step: 20 },
              input_uvlo_required_samples: { default: 3, min: 1, max: 5, step: 1 },
              source_limited_enter_delta_ma: {
                default: 2500,
                min: -100,
                max: 3000,
                step: 50,
              },
            },
          },
        }),
      ),
    ).toBe(true);
  });
});

describe("resolveOwnerFacingDevdTarget", () => {
  test("accepts explicit devd target values", () => {
    expect(resolveOwnerFacingDevdTarget(" ipc://devd.sock ", false)).toBe(
      "ipc://devd.sock",
    );
  });

  test("allows legacy mock target values in demo mode", () => {
    expect(resolveOwnerFacingDevdTarget("mock:devd", true)).toBe("mock:devd");
  });

  test("rejects mock target values outside demo mode", () => {
    expect(resolveOwnerFacingDevdTarget("mock:devd", false)).toBeUndefined();
  });
});

describe("resolveStartupDevdTarget", () => {
  test("prefers devd_target over legacy mock_devd_target", () => {
    const params = new URLSearchParams({
      devd_target: "ipc://preferred.sock",
      mock_devd_target: "ipc://legacy.sock",
    });
    expect(resolveStartupDevdTarget(params, false)).toBe(
      "ipc://preferred.sock",
    );
  });

  test("falls back to legacy mock_devd_target when devd_target is absent", () => {
    const params = new URLSearchParams({
      mock_devd_target: "ipc://legacy.sock",
    });
    expect(resolveStartupDevdTarget(params, false)).toBe("ipc://legacy.sock");
  });
});

describe("resolveDevdTarget", () => {
  test("keeps demo mode mock-only without an explicit devd target", () => {
    expect(resolveDevdTarget(undefined, false, true)).toBeNull();
  });
});

describe("resolveManualHttpRememberedChannel", () => {
  test("keeps verified hostnames as the primary remembered URL", () => {
    expect(
      resolveManualHttpRememberedChannel("mains-aegis-a1b2c3.local"),
    ).toEqual({
      rememberedHttpBaseUrl: "http://mains-aegis-a1b2c3.local",
    });
  });

  test("stores manual IPv4 targets as fallback URLs", () => {
    expect(resolveManualHttpRememberedChannel("192.168.31.42")).toEqual({
      rememberedHttpFallbackBaseUrl: "http://192.168.31.42",
    });
    expect(resolveManualHttpRememberedChannel("192.168.31.42:8080")).toEqual({
      rememberedHttpFallbackBaseUrl: "http://192.168.31.42:8080",
    });
  });
});
