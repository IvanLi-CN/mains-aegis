import {
  Activity,
  AlertTriangle,
  BellRing,
  BatteryFull,
  BatteryLow,
  BatteryMedium,
  BatteryWarning,
  BatteryCharging,
  BookOpen,
  Cable,
  CircleHelp,
  Cpu,
  FileDown,
  Gauge,
  GripHorizontal,
  Globe2,
  KeyRound,
  LayoutGrid,
  Loader2,
  Maximize2,
  Menu,
  Minimize2,
  ChevronDown,
  PlugZap,
  Radio,
  RefreshCw,
  Search,
  Server,
  Settings,
  SlidersHorizontal,
  Terminal,
  Thermometer,
  Trash2,
  Usb,
  Wifi,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import {
  FormEvent,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent,
  type ReactNode,
  type SVGProps,
} from "react";
import type { LucideIcon } from "lucide-react";
import * as Dialog from "@radix-ui/react-dialog";
import {
  bindDevdDevice,
  getDevdDeviceDiagSnapshot,
  getDevdDeviceAlerts,
  getDeviceAlerts,
  getIdentity,
  isHostedHttpServiceApp,
  isPublicStaticApp,
  listDevdDevices,
  normalizeBaseUrl,
  muteDevdDeviceAlert,
  muteDeviceAlert,
  releaseDevdTpsEnableInterlock,
  subscribeDevdDeviceEvents,
  toErrorEnvelope,
} from "../api/client";
import type {
  AdvancedPowerSettings,
  ActiveAlert,
  ActiveAlertsSnapshot,
  ChargeControlDetail,
  DeviceRecord,
  DeviceSettings,
  DevdDevice,
  Identity,
  LanCompanionCandidate,
  SerialLogEntry,
  SerialTraceEntry,
  TpsEnableInterlock,
  UpsStatus,
} from "../api/types";
import {
  buildAdvancedPowerDefaults,
  resolvePreTpsVinMv,
} from "../api/runtimeModeProfiles";
import { SegmentedControl } from "../components/ui/segmented-control";
import { Button } from "../components/ui/button";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../components/ui/select";
import {
  useDeviceRegistry,
  type AddDeviceInput,
  type DeviceChannelTransport,
  type ManualChargeControlInput,
  type WifiProvisioningProgress,
} from "../device-registry/context";
import { isDemoQueryEnabled } from "../demo/query";
import { demoSeedIds, type DemoSeed } from "../fixtures/mockDevices";
import { isWebSerialSupported } from "../serial/transport";
import {
  formatCurrent,
  formatPercent,
  formatTemp,
  formatVoltage,
  timeAgo,
} from "../utils/format";
import {
  deviceSeverity,
  modeLabel,
  severityRank,
  type Severity,
} from "../utils/severity";
import {
  captureTraceScrollAnchor,
  resolveAnchoredTraceScrollTop,
  type TraceScrollAnchor,
} from "./traceScrollAnchor";
import { FirmwarePage as FirmwarePageView } from "./firmware-page";

type Route = {
  path: string;
  deviceId: string | null;
  section:
    | "fleet"
    | "connect"
    | "overview"
    | "alerts"
    | "power"
    | "battery"
    | "thermal"
    | "device"
    | "firmware"
    | "settings"
    | "api";
};

type AppProps = {
  initialPath?: string;
  initialDevdTarget?: string;
  forceHostedHttpServiceApp?: boolean;
};

export type UiFeedback = {
  tone: "success" | "error";
  message: string;
};

type DiscoveredLogicalDevice = {
  key: string;
  deviceId: string | null;
  displayName: string;
  endpoint: string;
  existingRecord: DeviceRecord | null;
  pendingCompanionCandidate: LanCompanionCandidate | null;
  channels: Partial<
    Record<Extract<DeviceChannelTransport, "http" | "devd">, DevdDevice>
  >;
  availableTransports: Array<Extract<DeviceChannelTransport, "http" | "devd">>;
  connectionLabel: string;
  firmwareLabel: string;
  logLabel: string;
};

type BindTargetOption = {
  deviceId: string;
  label: string;
};

type FleetDeviceEntry = {
  key: string;
  record: DeviceRecord;
  saved: boolean;
};

type FleetDiscoveryStatus = "idle" | "checking" | "available" | "unavailable";

type ConnectRuntimeMode =
  | "hosted_devd"
  | "public_static"
  | "standalone_with_devd"
  | "standalone_no_devd";

type BrowserLanCapability = {
  supported: boolean;
  reason: string | null;
  chromeVersion: number | null;
};

type ScanCandidate = {
  key: string;
  deviceId: string;
  alias: string;
  endpoints: string[];
  baseUrl: string;
  mdnsBaseUrl: string;
  mdnsHost: string | null;
  fallbackBaseUrl: string | null;
  identity: Identity;
  existingRecord: DeviceRecord | null;
};

type ScanState = {
  status: "idle" | "scanning" | "done";
  cidr: string;
  message: UiFeedback | null;
  candidates: ScanCandidate[];
};

const demoSeedLabels: Record<DemoSeed, string> = {
  default: "Default fleet",
  dual: "Dual transport",
  empty: "Empty fleet",
  offline: "Offline device",
  large: "Large fleet",
  usb: "USB session",
  "power-headroom": "Power headroom",
  "power-watch": "Power watch",
  "power-limited": "Power limited",
  "power-cooldown": "Power cooldown",
};

type SharedDevdDiscovery = {
  devdTarget: string | null;
  devdDevices: DevdDevice[];
  status: FleetDiscoveryStatus;
  isRefreshing: boolean;
  lastUpdated: string | null;
  refresh: () => Promise<void>;
};

type UpsHardwareCapability = {
  outputProfile: string | null;
  ratedVoutMv: number | null;
  source: "identity" | "firmware" | "settings" | "unknown";
};

const deviceSections = [
  { id: "overview", label: "Overview", icon: Gauge },
  { id: "alerts", label: "Alerts", icon: BellRing },
  { id: "power", label: "Power", icon: PlugZap },
  { id: "battery", label: "Battery", icon: BatteryCharging },
  { id: "thermal", label: "Thermal", icon: Thermometer },
  { id: "device", label: "Device", icon: Cpu },
  { id: "firmware", label: "Firmware", icon: FileDown },
  { id: "settings", label: "Settings", icon: Settings },
  { id: "api", label: "API", icon: Cable },
] as const;

function deviceSectionLabel(section: Route["section"]): string {
  return deviceSections.find((item) => item.id === section)?.label ?? "Overview";
}

export type PagePresentation = {
  scope: "fleet" | "connect" | "device";
  title: string;
  showFleetSummary: boolean;
  showDeviceOverview: boolean;
};

export function resolvePagePresentation(
  section: Route["section"],
): PagePresentation {
  if (section === "fleet") {
    return {
      scope: "fleet",
      title: "UPS Fleet",
      showFleetSummary: true,
      showDeviceOverview: false,
    };
  }
  if (section === "connect") {
    return {
      scope: "connect",
      title: "Add device",
      showFleetSummary: false,
      showDeviceOverview: false,
    };
  }
  return {
    scope: "device",
    title: deviceSectionLabel(section),
    showFleetSummary: false,
    showDeviceOverview: section === "overview",
  };
}

export function resolveDeviceRouteSection(
  rawSection: string | undefined,
): Exclude<Route["section"], "fleet" | "connect"> {
  return (
    deviceSections.find((item) => item.id === rawSection)?.id ?? "overview"
  );
}

export function shouldShowDeviceDataContext(
  record: Pick<
    DeviceRecord,
    "connectionState" | "streamState" | "status" | "error"
  >,
): boolean {
  return (
    record.connectionState !== "online" ||
    record.streamState === "error" ||
    Boolean(record.error) ||
    !record.status
  );
}

export function resolveMobileNavContext(
  section: Route["section"],
  selected: DeviceRecord | null,
): string {
  const presentation = resolvePagePresentation(section);
  if (presentation.scope === "connect") return presentation.title;
  if (presentation.scope === "fleet") return "Fleet";
  if (!selected) return `Device / ${presentation.title}`;
  return `${selected.target.alias} / ${connectionSummary(selected)} / ${presentation.title}`;
}

const appBasePath = normalizeBasePath(
  import.meta.env.BASE_URL,
  runtimePathname(),
);
const envRuntimeMode = (import.meta.env.VITE_APP_RUNTIME_MODE ?? "").trim();
const rawEnvDevdTarget = (
  import.meta.env.VITE_DEFAULT_DEVD_URL ?? import.meta.env.VITE_DEVD_API_BASE ?? ""
).trim();
const envDevdTarget = rawEnvDevdTarget || "same-origin";
const docsHref = `${appBasePath}docs/`;
const lightBrandLogoAsset = "mains-aegis-logo-mark-color-light.svg";
const darkBrandLogoAsset = "mains-aegis-logo-mark-color-dark.svg";
const credentiallessInputProps = {
  autoComplete: "off",
  autoCorrect: "off",
  spellCheck: false,
  "data-1p-ignore": "true",
  "data-lpignore": "true",
  "data-form-type": "other",
} as const;

export function App({
  initialPath,
  initialDevdTarget,
  forceHostedHttpServiceApp,
}: AppProps = {}) {
  const registry = useDeviceRegistry();
  const route = useRoute(initialPath);
  const searchParams = new URLSearchParams(window.location.search);
  const demoMode = registry.demoSeed !== null || isDemoQueryEnabled();
  const demoTheme = resolveDemoTheme(searchParams, demoMode);
  useEffect(() => {
    if (!demoTheme) return;
    const root = document.documentElement;
    const previousTheme = root.getAttribute("data-theme");
    root.setAttribute("data-theme", demoTheme);
    return () => {
      if (previousTheme === null) root.removeAttribute("data-theme");
      else root.setAttribute("data-theme", previousTheme);
    };
  }, [demoTheme]);
  const queryDevdTarget = resolveStartupDevdTarget(searchParams, demoMode);
  const queryHostedHttpServiceApp =
    demoMode && searchParams.get("mock_hosted") === "1";
  const queryBindLogicalDeviceId = demoMode
    ? (searchParams.get("mock_bind_logical_device_id")?.trim() || "")
    : "";
  const brandLogoSrc = `${appBasePath}brand/mains-aegis/${resolveBrandLogoAsset(
    searchParams,
    demoMode,
    demoTheme,
  )}`;
  const resolvedInitialDevdTarget = initialDevdTarget ?? queryDevdTarget;
  const hostedHttpServiceApp =
    forceHostedHttpServiceApp ??
    (queryHostedHttpServiceApp || isHostedHttpServiceApp());
  const devdTarget = resolveDevdTarget(
    resolvedInitialDevdTarget,
    hostedHttpServiceApp,
    demoMode,
  );
  const devdDiscovery = useFleetDevdDiscovery(
    devdTarget,
    registry.rememberDiscoveredChannels,
  );
  const fleetEntries = useMemo(
    () =>
      buildFleetEntries(
        registry.records,
        devdDiscovery.devdDevices,
        devdDiscovery.devdTarget,
      ),
    [registry.records, devdDiscovery.devdDevices, devdDiscovery.devdTarget],
  );
  const fleetRecords = useMemo(
    () => fleetEntries.map((entry) => entry.record),
    [fleetEntries],
  );
  const fleetAlerts = useFleetActiveAlerts(fleetRecords);
  const registrySelected = route.deviceId
    ? (registry.records.find(
        (record) => record.target.deviceId === route.deviceId,
      ) ?? null)
    : null;
  const selected = resolveSelectedRecord(
    route.deviceId,
    registry.records,
    fleetEntries,
    devdDiscovery.devdTarget === null,
  );
  const activeAlerts = useActiveAlertsSnapshot(selected);
  const [navOpen, setNavOpen] = useState(false);
  const hydratedTemporaryDeviceIds = useRef(new Set<string>());

  useEffect(() => {
    setNavOpen(false);
  }, [route.path]);

  useEffect(() => {
    if (!route.deviceId || registrySelected) return;
    const fleetRecord =
      fleetEntries.find((entry) => entry.record.target.deviceId === route.deviceId)
        ?.record ?? null;
    if (!fleetRecord) return;
    registry.stageDeviceRecord({
      ...fleetRecord,
      target: {
        ...fleetRecord.target,
        temporary: true,
      },
    });
  }, [fleetEntries, registry, registrySelected, route.deviceId]);

  useEffect(() => {
    if (!route.deviceId || !registrySelected?.target.temporary) return;
    if (hydratedTemporaryDeviceIds.current.has(route.deviceId)) return;
    hydratedTemporaryDeviceIds.current.add(route.deviceId);
    void registry.refreshDevice(route.deviceId);
  }, [registry, registrySelected, route.deviceId]);

  useEffect(() => {
    if (!route.deviceId || !selected) return;
    if (selected.status && selected.connectionState !== "connecting") return;
    if (hydratedTemporaryDeviceIds.current.has(`prime:${route.deviceId}`)) return;
    hydratedTemporaryDeviceIds.current.add(`prime:${route.deviceId}`);
    void registry.refreshDevice(route.deviceId);
  }, [registry, route.deviceId, selected]);

  const pagePresentation = resolvePagePresentation(route.section);
  const mobileNavContext = resolveMobileNavContext(route.section, selected);

  return (
    <div className="app-shell">
      <aside className={`sidebar ${navOpen ? "is-open" : ""}`}>
        <div className="mobile-nav-bar">
          <button
            className="icon-button"
            type="button"
            aria-label={navOpen ? "Close navigation" : "Open navigation"}
            aria-expanded={navOpen}
            aria-controls="sidebar-navigation"
            onClick={() => setNavOpen((open) => !open)}
          >
            {navOpen ? <X size={18} /> : <Menu size={18} />}
          </button>
          <div className="mobile-nav-title">
            <strong>Mains Aegis</strong>
            {pagePresentation.scope === "device" && selected ? (
              <span className="mobile-nav-context" title={mobileNavContext}>
                <span className="mobile-nav-device">
                  {selected.target.alias}
                </span>
                <span className="mobile-nav-divider" aria-hidden="true">
                  /
                </span>
                <span className="mobile-nav-route">
                  {connectionSummary(selected)} / {pagePresentation.title}
                </span>
              </span>
            ) : (
              <span className="mobile-nav-context" title={mobileNavContext}>
                {mobileNavContext}
              </span>
            )}
          </div>
        </div>
        <button
          className="mobile-nav-backdrop"
          type="button"
          aria-label="Close navigation"
          onClick={() => setNavOpen(false)}
        />
        <div id="sidebar-navigation" className="sidebar-panel">
          <div className={`brand ${demoMode ? "is-demo" : ""}`}>
            {demoMode ? (
              <DemoControlPanel
                seed={registry.demoSeed ?? "default"}
                brandLogoSrc={brandLogoSrc}
                onSeedChange={registry.setDemoSeed}
                onReset={registry.resetDemo}
              />
            ) : (
              <span className="brand-mark" aria-hidden="true">
                <img src={brandLogoSrc} alt="" />
              </span>
            )}
            <div>
              <strong>Mains Aegis</strong>
              <span>{demoMode ? "Demo fleet console" : "UPS fleet console"}</span>
            </div>
          </div>

          <nav className="nav-group" aria-label="Fleet navigation">
            <NavLink
              href="/"
              active={route.section === "fleet"}
              icon={LayoutGrid}
              label="Fleet"
            />
            <NavLink
              href="/connect"
              active={route.section === "connect"}
              icon={Wifi}
              label="Add device"
            />
            <ExternalNavLink href={docsHref} icon={BookOpen} label="Docs" />
          </nav>

          {selected ? (
            <nav className="nav-group" aria-label="Device navigation">
              <div className="nav-caption">{selected.target.alias}</div>
              {deviceSections.map((section) => (
                <NavLink
                  key={section.id}
                  href={deviceHref(selected.target.deviceId, section.id)}
                  active={route.section === section.id}
                  icon={section.icon}
                  label={section.label}
                />
              ))}
            </nav>
          ) : null}
        </div>
      </aside>

      <main
        className={`main-surface ${route.section === "connect" ? "connect-adapt-command" : ""}`}
      >
        {pagePresentation.showFleetSummary ? (
          <FleetHeader
            records={fleetRecords}
            activeAlertsByDevice={fleetAlerts.snapshots}
          />
        ) : null}
        {renderRoute(
          route,
          fleetEntries,
          selected,
          resolvedInitialDevdTarget || undefined,
          hostedHttpServiceApp,
          devdDiscovery,
          activeAlerts,
        )}
      </main>
    </div>
  );
}

function renderRoute(
  route: Route,
  fleetEntries: FleetDeviceEntry[],
  selected: DeviceRecord | null,
  initialDevdTarget?: string,
  hostedHttpServiceApp?: boolean,
  devdDiscovery?: SharedDevdDiscovery,
  activeAlerts?: ActiveAlertsViewState,
) {
  const pagePresentation = resolvePagePresentation(route.section);
  if (route.section === "connect") {
    return (
      <ConnectPage
        initialDevdTarget={initialDevdTarget}
        hostedHttpServiceApp={hostedHttpServiceApp}
        sharedDevdDiscovery={devdDiscovery}
      />
    );
  }
  if (!route.deviceId && devdDiscovery) {
    return <FleetPage entries={fleetEntries} discovery={devdDiscovery} />;
  }
  if (
    route.deviceId &&
    !selected &&
    devdDiscovery &&
    (devdDiscovery.status === "checking" || devdDiscovery.isRefreshing)
  ) {
    return <DeviceRoutePlaceholder title={pagePresentation.title} state="loading" />;
  }
  if (!selected) {
    return <DeviceRoutePlaceholder title={pagePresentation.title} state="missing" />;
  }

  let content: ReactNode;
  switch (route.section) {
    case "alerts":
      content = <AlertsPage record={selected} state={activeAlerts!} />;
      break;
    case "power":
      content = <PowerPage record={selected} />;
      break;
    case "battery":
      content = <BatteryPage record={selected} />;
      break;
    case "thermal":
      content = <ThermalPage record={selected} />;
      break;
    case "device":
      content = <DeviceInfoPage record={selected} />;
      break;
    case "firmware":
      content = <FirmwarePageView record={selected} />;
      break;
    case "settings":
      content = <SettingsPage record={selected} />;
      break;
    case "api":
      content = <ApiDebugPage record={selected} />;
      break;
    default:
      content = <DeviceOverviewPage record={selected} />;
  }

  return (
    <DevicePageFrame presentation={pagePresentation} record={selected}>
      {content}
    </DevicePageFrame>
  );
}

export function DevicePageFrame({
  presentation,
  record,
  children,
}: {
  presentation: PagePresentation;
  record: DeviceRecord;
  children: ReactNode;
}) {
  return (
    <div className="device-page-frame" data-evidence-target="device-page">
      <header className="page-header" data-evidence-target="device-page-header">
        <div>
          <div className="eyebrow">{record.target.alias}</div>
          <h1>{presentation.title}</h1>
        </div>
      </header>
      {presentation.showDeviceOverview ? (
        <DeviceStatusBand record={record} />
      ) : shouldShowDeviceDataContext(record) ? (
        <DeviceDataContext record={record} />
      ) : null}
      {children}
    </div>
  );
}

export function DeviceDataContext({ record }: { record: DeviceRecord }) {
  const stream = streamPresentation(record);
  return (
    <div className={`device-data-context tone-${stream.tone}`} role="status">
      <span className="eyebrow">Data state</span>
      <strong>{stream.label}</strong>
      <span>{stream.detail}</span>
    </div>
  );
}

function useRoute(initialPath?: string): Route {
  const readPath = () => stripBasePath(initialPath ?? window.location.pathname);
  const [path, setPath] = useState(readPath);

  useEffect(() => {
    if (initialPath) setPath(stripBasePath(initialPath));
  }, [initialPath]);

  useEffect(() => {
    const listener = () => setPath(stripBasePath(window.location.pathname));
    window.addEventListener("popstate", listener);
    return () => window.removeEventListener("popstate", listener);
  }, []);

  return parseRoute(path);
}

export function resolveDevdTarget(
  initialDevdTarget: string | undefined,
  hostedHttpServiceApp: boolean,
  demoMode: boolean,
): string | null {
  if (demoMode && !hostedHttpServiceApp && !initialDevdTarget && !rawEnvDevdTarget)
    return null;
  if (
    (isPublicStaticApp() || envRuntimeMode === "public_static") &&
    !initialDevdTarget &&
    !rawEnvDevdTarget
  )
    return null;
  const candidate = (
    initialDevdTarget ??
    (hostedHttpServiceApp ? "same-origin" : envDevdTarget)
  ).trim();
  if (!candidate) return null;
  return candidate;
}

function ownerFacingDevdTargetParam(
  searchParams: URLSearchParams,
): string | null {
  return searchParams.get("devd_target") ?? searchParams.get("mock_devd_target");
}

export function resolveStartupDevdTarget(
  searchParams: URLSearchParams,
  demoMode: boolean,
): string | undefined {
  return resolveOwnerFacingDevdTarget(
    ownerFacingDevdTargetParam(searchParams),
    demoMode,
  );
}

export function resolveOwnerFacingDevdTarget(
  value: string | null | undefined,
  demoMode: boolean,
): string | undefined {
  const candidate = value?.trim();
  if (!candidate) return undefined;
  if (!demoMode && candidate.startsWith("mock:")) return undefined;
  return candidate;
}

export function resolveConnectRuntimeMode(options: {
  hostedHttpServiceApp: boolean;
  devdTarget: string | null;
  publicStaticBuild?: boolean;
}): ConnectRuntimeMode {
  if (options.hostedHttpServiceApp) return "hosted_devd";
  const publicStaticBuild =
    options.publicStaticBuild ??
    (isPublicStaticApp() || envRuntimeMode === "public_static");
  if (publicStaticBuild) {
    return options.devdTarget ? "standalone_with_devd" : "public_static";
  }
  return options.devdTarget ? "standalone_with_devd" : "standalone_no_devd";
}

export function detectBrowserLanCapability(
  options: {
    isSecureContext?: boolean;
    userAgent?: string;
  } = {},
): BrowserLanCapability {
  if (typeof navigator === "undefined" && options.userAgent === undefined) {
    return {
      supported: false,
      reason: "Browser capability checks are unavailable in this environment.",
      chromeVersion: null,
    };
  }
  const secureContext =
    options.isSecureContext ??
    (typeof window !== "undefined" ? window.isSecureContext : false);
  if (!secureContext) {
    return {
      supported: false,
      reason: "Secure context is required for browser-direct LAN access.",
      chromeVersion: null,
    };
  }
  const ua = options.userAgent ?? navigator.userAgent;
  const chromeMatch = ua.match(/Chrom(?:e|ium)\/(\d+)/);
  const chromeVersion = chromeMatch
    ? Number.parseInt(chromeMatch[1] ?? "", 10)
    : null;
  if (!chromeVersion) {
    return {
      supported: false,
      reason: "Use Chrome 142+ for GitHub Pages LAN access.",
      chromeVersion: null,
    };
  }
  if (chromeVersion < 142) {
    return {
      supported: false,
      reason: `Chrome 142+ is required; detected Chrome ${chromeVersion}.`,
      chromeVersion,
    };
  }
  return { supported: true, reason: null, chromeVersion };
}

function parseIpv4Address(input: string): number | null {
  const parts = input.trim().split(".");
  if (parts.length !== 4) return null;
  const octets = parts.map((part) => Number.parseInt(part, 10));
  if (
    octets.some(
      (octet, index) =>
        Number.isNaN(octet) ||
        octet < 0 ||
        octet > 255 ||
        `${octet}` !== parts[index],
    )
  ) {
    return null;
  }
  return (
    ((octets[0] ?? 0) << 24) >>> 0 |
    ((octets[1] ?? 0) << 16) |
    ((octets[2] ?? 0) << 8) |
    (octets[3] ?? 0)
  ) >>> 0;
}

function ipv4NumberToString(value: number): string {
  return [
    (value >>> 24) & 255,
    (value >>> 16) & 255,
    (value >>> 8) & 255,
    value & 255,
  ].join(".");
}

export function expandIpv4Cidr(input: string): {
  hosts: string[];
  normalized: string;
} {
  const raw = input.trim();
  const [ipText, prefixText] = raw.split("/", 2);
  const ip = parseIpv4Address(ipText ?? "");
  const prefix = Number.parseInt(prefixText ?? "", 10);
  if (ip === null || Number.isNaN(prefix) || prefix < 0 || prefix > 32) {
    throw new Error("Use a valid IPv4 CIDR.");
  }
  const hostBits = 32 - prefix;
  const blockSize = 2 ** hostBits;
  const hostCount = blockSize - 2;
  if (hostCount < 2 || hostCount > 256) {
    throw new Error("CIDR scan must expand to between 2 and 256 hosts.");
  }
  const mask = prefix === 0 ? 0 : (0xffffffff << hostBits) >>> 0;
  const network = ip & mask;
  const hosts: string[] = [];
  for (
    let current = network + 1;
    current < network + blockSize - 1;
    current += 1
  ) {
    hosts.push(ipv4NumberToString(current >>> 0));
  }
  return {
    hosts,
    normalized: `${ipv4NumberToString(network >>> 0)}/${prefix}`,
  };
}

function isIpv4Host(value: string): boolean {
  const [host] = value.trim().split(":", 2);
  return parseIpv4Address(host ?? "") !== null;
}

export function resolveManualHttpRememberedChannel(
  target: string,
): Partial<
  Pick<
  AddDeviceInput,
  "rememberedHttpBaseUrl" | "rememberedHttpFallbackBaseUrl"
  >
> {
  const normalizedTarget = normalizeBaseUrl(target);
  if (!normalizedTarget) return {};
  const host = normalizedTarget.replace(/^https?:\/\//i, "").replace(/\/+$/, "");
  if (isIpv4Host(host)) {
    return { rememberedHttpFallbackBaseUrl: normalizedTarget };
  }
  return { rememberedHttpBaseUrl: normalizedTarget };
}

export function isLanIdentityCandidate(identity: Identity): boolean {
  return (
    identity.role === "ups" &&
    identity.api_version === "v1" &&
    identity.device_id.trim().length > 0
  );
}

function useFleetDevdDiscovery(
  devdTarget: string | null,
  rememberDiscoveredChannels: (
    devdBaseUrl: string,
    devices: DevdDevice[],
  ) => void,
) {
  const [devdDevices, setDevdDevices] = useState<DevdDevice[]>([]);
  const [status, setStatus] = useState<FleetDiscoveryStatus>(
    devdTarget ? "checking" : "idle",
  );
  const [isRefreshing, setIsRefreshing] = useState(Boolean(devdTarget));
  const [lastUpdated, setLastUpdated] = useState<string | null>(null);
  const hasDiscoverySnapshot = useRef(false);
  const discoveryRequestGeneration = useRef(0);

  const filterDevices = useCallback(
    (devices: DevdDevice[]) =>
      devices.filter(
        (device) =>
          device.transport !== "lan" || isMainsAegisLanDevice(device),
      ),
    [],
  );

  const applyDiscoverySnapshot = useCallback(
    (devdBaseUrl: string, devices: DevdDevice[]) => {
      const filteredDevices = filterDevices(devices);
      setDevdDevices(filteredDevices);
      rememberDiscoveredChannels(devdBaseUrl, filteredDevices);
      hasDiscoverySnapshot.current = true;
      setStatus("available");
      setLastUpdated(new Date().toISOString());
    },
    [filterDevices, rememberDiscoveredChannels],
  );

  const refreshDiscovery = useCallback(async () => {
    const requestGeneration = discoveryRequestGeneration.current + 1;
    discoveryRequestGeneration.current = requestGeneration;
    if (!devdTarget) {
      hasDiscoverySnapshot.current = false;
      setDevdDevices([]);
      setStatus("idle");
      setIsRefreshing(false);
      setLastUpdated(null);
      return;
    }
    const devdBaseUrl = normalizeBaseUrl(devdTarget);
    setIsRefreshing(true);
    try {
      const devices = await listDevdDevices(devdBaseUrl);
      if (requestGeneration !== discoveryRequestGeneration.current) return;
      applyDiscoverySnapshot(devdBaseUrl, devices.devices);
    } catch {
      if (requestGeneration !== discoveryRequestGeneration.current) return;
      setStatus(hasDiscoverySnapshot.current ? "available" : "unavailable");
      if (!hasDiscoverySnapshot.current) {
        setDevdDevices([]);
        setLastUpdated(null);
      }
    } finally {
      if (requestGeneration === discoveryRequestGeneration.current)
        setIsRefreshing(false);
    }
  }, [applyDiscoverySnapshot, devdTarget]);

  useEffect(() => {
    hasDiscoverySnapshot.current = false;
    void refreshDiscovery();
  }, [refreshDiscovery]);

  useEffect(() => {
    if (!devdTarget) return undefined;
    const interval = window.setInterval(() => void refreshDiscovery(), 10000);
    return () => window.clearInterval(interval);
  }, [devdTarget, refreshDiscovery]);

  useEffect(() => {
    if (!devdTarget || status !== "available") return undefined;
    const devdBaseUrl = normalizeBaseUrl(devdTarget);
    const stream = subscribeDevdDeviceEvents(devdBaseUrl, {
      onEvent: () => void refreshDiscovery(),
      onError: () => undefined,
    });
    return () => stream.close();
  }, [devdTarget, refreshDiscovery, status]);

  return {
    devdTarget,
    devdDevices,
    status,
    isRefreshing,
    lastUpdated,
    refresh: refreshDiscovery,
  };
}

function parseRoute(path: string): Route {
  if (path === "/connect") return { path, deviceId: null, section: "connect" };
  const match = path.match(/^\/devices\/([^/]+)(?:\/([^/]+))?$/);
  if (match) {
    const section = resolveDeviceRouteSection(match[2]);
    return { path, deviceId: decodeURIComponent(match[1]), section };
  }
  return { path, deviceId: null, section: "fleet" };
}

function navigate(path: string) {
  const next = new URL(withBasePath(path), window.location.origin);
  const currentSearch = new URLSearchParams(window.location.search);
  if (!next.search && currentSearch.get("demo") === "true") {
    next.searchParams.set("demo", "true");
    const theme = currentSearch.get("theme")?.trim();
    if (theme) next.searchParams.set("theme", theme);
  }
  window.history.pushState(
    null,
    "",
    `${next.pathname}${next.search}${next.hash}`,
  );
  window.dispatchEvent(new PopStateEvent("popstate"));
}

export function resolveBrandLogoAsset(
  _searchParams: URLSearchParams,
  _demoMode: boolean,
  demoTheme: "light" | "dark" | null = null,
): string {
  return demoTheme === "dark" ? darkBrandLogoAsset : lightBrandLogoAsset;
}

export function resolveDemoTheme(
  searchParams: URLSearchParams,
  demoMode: boolean,
): "light" | "dark" | null {
  if (!demoMode) return null;
  const requested = searchParams.get("theme")?.trim();
  return requested === "light" || requested === "dark" ? requested : null;
}

export function normalizeBasePath(
  base: string | undefined,
  runtimePathname = "/",
): string {
  const raw = (base ?? "").trim();
  if (!raw || raw === "/") return "/";
  if (raw === "." || raw === "./")
    return normalizeRuntimeBasePath(runtimePathname);
  if (!raw.startsWith("/") && /^[a-z][a-z0-9+.-]*:/i.test(raw)) return "/";
  if (raw.startsWith("./") || raw.startsWith("../"))
    return normalizeRuntimeBasePath(runtimePathname);
  const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

function normalizeRuntimeBasePath(pathname: string): string {
  const pathnameOnly = pathname.split("?", 1)[0]?.split("#", 1)[0] || "/";
  const withLeading = pathnameOnly.startsWith("/") ? pathnameOnly : `/${pathnameOnly}`;
  const rawSegments = withLeading.split("/").filter(Boolean);
  const lastSegment = rawSegments.at(-1);
  const hasIndexLikeEntry = lastSegment
    ? ["index.html", "404.html"].includes(lastSegment)
    : false;
  const segments = hasIndexLikeEntry ? rawSegments.slice(0, -1) : rawSegments;
  if (segments.length === 0) return "/";
  const routeIndex = segments.findIndex((segment) =>
    ["connect", "devices", "docs"].includes(segment),
  );
  if (routeIndex === 0) return "/";
  if (routeIndex > 0) return `/${segments.slice(0, routeIndex).join("/")}/`;
  if (hasIndexLikeEntry) return `/${segments.join("/")}/`;
  if (withLeading.endsWith("/")) return `/${segments.join("/")}/`;
  if (segments.length === 2) return `/${segments.join("/")}/`;
  if (segments.length === 1)
    return withLeading.endsWith("/") ? `/${segments[0]}/` : "/";
  return "/";
}

function runtimePathname(): string {
  return typeof window === "undefined" ? "/" : window.location.pathname;
}

function stripBasePath(path: string): string {
  const pathname = path.startsWith("/") ? path : `/${path}`;
  if (appBasePath === "/") return pathname;
  const baseWithoutTrailingSlash = appBasePath.slice(0, -1);
  if (pathname === baseWithoutTrailingSlash) return "/";
  if (pathname.startsWith(appBasePath))
    return pathname.slice(baseWithoutTrailingSlash.length) || "/";
  return pathname;
}

function withBasePath(path: string): string {
  if (appBasePath === "/") return path;
  const pathname = path.startsWith("/") ? path : `/${path}`;
  return `${appBasePath.slice(0, -1)}${pathname}`;
}

function deviceHref(deviceId: string, section: string) {
  return section === "overview"
    ? `/devices/${encodeURIComponent(deviceId)}`
    : `/devices/${encodeURIComponent(deviceId)}/${section}`;
}

function deviceDefaultHref(record: DeviceRecord) {
  const preferred =
    activeRecordTransport(record) ?? preferredRecordTransport(record);
  return deviceHref(
    record.target.deviceId,
    preferred === "serial" || preferred === "devd" ? "firmware" : "overview",
  );
}

function hasTransportFailure(record: DeviceRecord | null | undefined): boolean {
  return Boolean(
    record &&
      record.errorSource !== "read" &&
      (record.connectionState === "error" ||
        record.streamState === "error" ||
        (record.connectionState === "offline" && record.error !== null)),
  );
}

export function resolveSelectedRecord(
  deviceId: string | null,
  records: DeviceRecord[],
  fleetEntries: FleetDeviceEntry[],
  allowTemporaryRegistry = true,
) {
  if (!deviceId) return null;
  const registryRecord =
    records.find((record) => record.target.deviceId === deviceId) ?? null;
  const fleetRecord =
    fleetEntries.find((entry) => entry.record.target.deviceId === deviceId)
      ?.record ?? null;
  if (!registryRecord) return fleetRecord;
  if (!fleetRecord) {
    return registryRecord.target.temporary && !allowTemporaryRegistry
      ? null
      : registryRecord;
  }
  const registryActionFailure =
    registryRecord.target.transport === "http" &&
    registryRecord.error &&
    !hasTransportFailure(registryRecord);
  if (
    (hasTransportFailure(registryRecord) || registryActionFailure) &&
    !hasTransportFailure(fleetRecord)
  ) {
    return fleetRecord;
  }
  if (registryRecord.target.temporary) return fleetRecord;
  if (
    registryRecord.target.temporary &&
    !registryRecord.status &&
    fleetRecord.status
  ) {
    return fleetRecord;
  }
  return registryRecord;
}

function DemoControlPanel({
  seed,
  brandLogoSrc,
  onSeedChange,
  onReset,
}: {
  seed: DemoSeed;
  brandLogoSrc: string;
  onSeedChange: (seed: DemoSeed) => void;
  onReset: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ x: 276, y: 76 });
  const dragOffset = useRef<{ x: number; y: number } | null>(null);
  const scenarioId = useId();
  const panelStyle: CSSProperties = {
    left: position.x,
    top: position.y,
  };

  const startDrag = (event: PointerEvent<HTMLElement>) => {
    dragOffset.current = {
      x: event.clientX - position.x,
      y: event.clientY - position.y,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const drag = (event: PointerEvent<HTMLElement>) => {
    const offset = dragOffset.current;
    if (!offset) return;
    const maxX = Math.max(16, window.innerWidth - 336);
    const maxY = Math.max(16, window.innerHeight - 260);
    setPosition({
      x: Math.min(Math.max(16, event.clientX - offset.x), maxX),
      y: Math.min(Math.max(16, event.clientY - offset.y), maxY),
    });
  };

  const stopDrag = (event: PointerEvent<HTMLElement>) => {
    dragOffset.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId))
      event.currentTarget.releasePointerCapture(event.pointerId);
  };

  return (
    <div className="demo-brand-control">
      <button
        className="brand-mark demo-logo"
        type="button"
        aria-expanded={open}
        aria-label="Open demo control panel"
        onClick={() => setOpen((current) => !current)}
      >
        <img src={brandLogoSrc} alt="" aria-hidden="true" />
      </button>
      {open ? (
        <section
          className="demo-control-panel"
          style={panelStyle}
          aria-label="Demo control panel"
        >
          <header className="demo-control-header">
            <button
              className="demo-control-drag-handle"
              type="button"
              aria-label="Move demo control panel"
              onPointerDown={startDrag}
              onPointerMove={drag}
              onPointerUp={stopDrag}
              onPointerCancel={stopDrag}
            >
              <GripHorizontal size={16} aria-hidden="true" />
            </button>
            <div>
              <strong>Demo Control</strong>
              <span>{demoSeedLabels[seed]}</span>
            </div>
            <Button
              className="demo-control-close"
              variant="ghost"
              size="icon"
              type="button"
              aria-label="Close demo control panel"
              onClick={() => setOpen(false)}
            >
              <X size={15} />
            </Button>
          </header>
          <div className="demo-control-field">
            <span id={scenarioId}>Scenario</span>
            <Select
              value={seed}
              onValueChange={(value) => onSeedChange(value as DemoSeed)}
            >
              <SelectTrigger aria-labelledby={scenarioId}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {demoSeedIds.map((value) => (
                    <SelectItem key={value} value={value}>
                      {demoSeedLabels[value]}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>
          <div className="demo-control-actions">
            <Button variant="secondary" size="sm" type="button" onClick={onReset}>
              Reset default
            </Button>
          </div>
        </section>
      ) : null}
    </div>
  );
}

function NavLink({
  href,
  active,
  icon: Icon,
  label,
}: {
  href: string;
  active: boolean;
  icon: LucideIcon;
  label: string;
}) {
  const publicHref = withBasePath(href);
  return (
    <a
      className={`nav-link ${active ? "is-active" : ""}`}
      href={publicHref}
      onClick={(event) => {
        event.preventDefault();
        navigate(href);
      }}
    >
      <Icon size={17} />
      <span>{label}</span>
    </a>
  );
}

function ExternalNavLink({
  href,
  icon: Icon,
  label,
}: {
  href: string;
  icon: LucideIcon;
  label: string;
}) {
  return (
    <a className="nav-link" href={href} target="_blank" rel="noreferrer">
      <Icon size={17} />
      <span>{label}</span>
    </a>
  );
}

export function FleetHeader({
  records,
  activeAlertsByDevice,
}: {
  records: DeviceRecord[];
  activeAlertsByDevice: Record<string, ActiveAlertsSnapshot>;
}) {
  const counts = useMemo(() => {
    const severities = records.map(
      (record) =>
        activeAlertSeverity(activeAlertsByDevice[record.target.deviceId] ?? null),
    );
    return {
      total: records.length,
      online: records.filter((record) => record.connectionState === "online")
        .length,
      critical: severities.filter((severity) => severity === "critical").length,
      warning: severities.filter((severity) => severity === "warning").length,
      offline: records.filter((record) => deviceSeverity(record) === "offline").length,
    };
  }, [activeAlertsByDevice, records]);

  const alertTargetDeviceId = (severity: "critical" | "warning") =>
    records.find(
      (record) =>
        activeAlertSeverity(activeAlertsByDevice[record.target.deviceId] ?? null) ===
        severity,
    )?.target.deviceId ?? null;
  const criticalTarget = alertTargetDeviceId("critical");
  const warningTarget = alertTargetDeviceId("warning");

  return (
    <header className="topbar fleet-header" data-evidence-target="fleet-summary">
      <div>
        <div className="eyebrow">Fleet</div>
        <h1>UPS Fleet</h1>
      </div>
      <div className="topbar-metrics">
        <Metric label="Total" value={counts.total} />
        <Metric label="Online" value={counts.online} />
        <Metric
          label="Critical"
          value={counts.critical}
          tone={counts.critical > 0 ? "critical" : "ok"}
          onClick={
            counts.critical > 0 && criticalTarget
              ? () => navigate(deviceHref(criticalTarget, "alerts"))
              : undefined
          }
        />
        <Metric
          label="Warning"
          value={counts.warning}
          tone={counts.warning > 0 ? "warning" : "ok"}
          onClick={
            counts.warning > 0 && warningTarget
              ? () => navigate(deviceHref(warningTarget, "alerts"))
              : undefined
          }
        />
        <Metric
          label="Offline"
          value={counts.offline}
          tone={counts.offline > 0 ? "offline" : "ok"}
        />
      </div>
    </header>
  );
}

function FleetPage({
  entries,
  discovery,
}: {
  entries: FleetDeviceEntry[];
  discovery: SharedDevdDiscovery;
}) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Severity | "all">("all");
  const initialLoading =
    discovery.status === "checking" &&
    discovery.isRefreshing &&
    entries.length === 0;
  const showRefreshingHint =
    discovery.isRefreshing &&
    entries.length > 0 &&
    discovery.devdTarget !== null;
  const filtered = entries
    .filter((entry) => {
      const record = entry.record;
      const haystack =
        `${record.target.alias} ${record.target.location} ${record.identity?.hostname ?? record.target.deviceId}`.toLowerCase();
      const matchesQuery = haystack.includes(query.toLowerCase());
      const matchesFilter =
        filter === "all" || deviceSeverity(record) === filter;
      return matchesQuery && matchesFilter;
    })
    .sort(
      (left, right) =>
        severityRank(deviceSeverity(left.record)) -
          severityRank(deviceSeverity(right.record)) ||
        left.record.target.alias.localeCompare(right.record.target.alias),
    );

  return (
    <section className="page-flow">
      <div className="toolbar">
        <label className="search-box">
          <Search size={16} />
          <input
            name="fleet-search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search device, hostname, location"
          />
        </label>
        <SegmentedControl
          label="Fleet filter"
          value={filter}
          options={[
            ["all", "all"],
            ["critical", "critical"],
            ["warning", "warning"],
            ["offline", "offline"],
          ]}
          onChange={setFilter}
        />
      </div>

      {showRefreshingHint ? (
        <div className="fleet-loading-hint" role="status" aria-live="polite">
          <Loader2 size={16} className="spin-icon" />
          <span>Refreshing device records from mains-aegis-devd</span>
        </div>
      ) : null}

      <div className="fleet-grid" data-evidence-target="fleet-grid">
        {initialLoading ? (
          <FleetLoadingState />
        ) : filtered.length > 0 ? (
          filtered.map((entry) => (
            <DeviceCard key={entry.key} entry={entry} />
          ))
        ) : (
          <FleetEmptyState hasDevices={entries.length > 0} />
        )}
      </div>
    </section>
  );
}

function FleetLoadingState() {
  return (
    <section className="empty-state fleet-empty fleet-loading-state">
      <Loader2 size={28} className="spin-icon" />
      <h2>Loading UPS fleet</h2>
      <p>
        Loading saved devices and current mains-aegis-devd device records
        before the fleet view renders.
      </p>
    </section>
  );
}

function FleetEmptyState({ hasDevices }: { hasDevices: boolean }) {
  return (
    <section className="empty-state fleet-empty">
      <Server size={28} />
      <h2>{hasDevices ? "No matching devices" : "No UPS devices available"}</h2>
      <p>
        {hasDevices
          ? "Adjust the search or status filter to bring devices back into view."
          : "No device is saved in this browser, and mains-aegis-devd is not reporting any current device records. Open the add-device page to review devd records or save a device."}
      </p>
      <button
        className="primary-button"
        type="button"
        onClick={() => navigate("/connect")}
      >
        Add device
      </button>
    </section>
  );
}

function DeviceCard({ entry }: { entry: FleetDeviceEntry }) {
  const { record, saved } = entry;
  const severity = deviceSeverity(record);
  const status = record.status;

  return (
    <article className={`device-card severity-${severity}`}>
      <div className="card-topline">
        <div>
          <h2>{record.target.alias}</h2>
          <p>{record.identity?.hostname ?? record.target.deviceId}</p>
        </div>
        <span className={`status-dot ${record.connectionState}`} />
      </div>

      <div className="mode-row">
        <span className={`mode-pill mode-${status?.mode ?? "unknown"}`}>
          {modeLabel(status?.mode)}
        </span>
        <SeverityBadge severity={severity} />
        <ConnectionBadges record={record} />
        {!saved ? <span className="transport-badge devd">devd record</span> : null}
      </div>

      <div className="card-main card-main-icon-duo metric-duo-stack">
        <div className="metric-tile">
          <span className="metric-icon metric-icon-size-a">
            <BatteryLevelIcon status={status} />
          </span>
          <span className="metric-copy">
            <span className="metric-label">Battery</span>
            <strong>{formatPercent(status?.battery.soc_pct)}</strong>
          </span>
        </div>
        <div className="metric-tile">
          <span className="metric-icon metric-icon-size-a">
            <PowerMetricIcon record={record} />
          </span>
          <span className="metric-copy">
            <span className="metric-label">Power</span>
            <strong>{powerSourceLabel(record)}</strong>
          </span>
        </div>
      </div>

      <dl className="summary-list">
        <StatusPair label="Load" value={loadSummary(record)} />
        <StatusPair label="Profile" value={hardwareCapabilitySummary(record)} />
        <StatusPair label="Battery" value={batterySummary(record)} />
        <StatusPair label="Attention" value={attentionSummary(record)} />
        <StatusPair label="Connection" value={connectionSummary(record)} />
      </dl>

      <div className="card-footer">
        <span>
          {record.target.location} · {timeAgo(record.lastUpdated)}
        </span>
        <div>
          <button
            className="primary-button small"
            type="button"
            aria-label={`Open ${record.target.alias}`}
            onClick={() => navigate(deviceHref(record.target.deviceId, "overview"))}
          >
            Open
          </button>
        </div>
      </div>
    </article>
  );
}

function ConnectionBadges({ record }: { record: DeviceRecord }) {
  const active = activeRecordTransport(record);
  const channels = availableRecordChannels(record);
  return (
    <span className="connection-badges">
      {channels.map((transport) => (
        <span
          className={`transport-badge ${transportBadgeClass(transport)}`}
          key={transport}
        >
          {transport === active
            ? channelBadgeLabel(transport)
            : channelBadgeLabel(transport)}
        </span>
      ))}
      {active === null ? (
        <span className="transport-badge offline">Offline</span>
      ) : null}
    </span>
  );
}

function activeRecordTransport(
  record: DeviceRecord,
): DeviceChannelTransport | null {
  if (record.serial?.connected)
    return record.serial.source === "devd" ? "devd" : "serial";
  if (record.target.transport === "devd" && record.connectionState === "online")
    return "devd";
  if (
    (record.target.transport ?? "http") === "http" &&
    (record.connectionState === "online" ||
      record.network?.state === "connected" ||
      record.status?.network.state === "connected")
  ) {
    return "http";
  }
  if (
    record.target.transport === "serial" &&
    record.connectionState === "online"
  )
    return "serial";
  return null;
}

export function deviceSettingsAvailable(record: DeviceRecord): boolean {
  const activeTransport = activeRecordTransport(record);
  if (activeTransport === "serial") return Boolean(record.settings);
  if (activeTransport === "http") return Boolean(record.settings);
  if (activeTransport === "devd") return Boolean(record.settings);
  return false;
}

function preferredRecordTransport(
  record: DeviceRecord,
): DeviceChannelTransport {
  const preferred = record.target.preferredTransport;
  if (preferred && hasRememberedChannel(record, preferred)) return preferred;
  if (hasRememberedChannel(record, "devd")) return "devd";
  if (hasRememberedChannel(record, "http")) return "http";
  return "serial";
}

function availableRecordChannels(
  record: DeviceRecord,
): DeviceChannelTransport[] {
  return sortTransportsByPreference(
    (["devd", "http", "serial"] as DeviceChannelTransport[]).filter(
      (transport) => hasRememberedChannel(record, transport),
    ),
    preferredRecordTransport(record),
  );
}

function hasRememberedChannel(
  record: DeviceRecord,
  transport: DeviceChannelTransport,
): boolean {
  if (transport === "http")
    return Boolean(
      record.target.rememberedChannels?.http ||
      (record.target.transport ?? "http") === "http",
    );
  if (transport === "devd")
    return Boolean(
      record.target.rememberedChannels?.devd ||
      record.target.transport === "devd" ||
      record.serial?.source === "devd",
    );
  return Boolean(
    record.target.rememberedChannels?.serial ||
    record.target.transport === "serial" ||
    (record.serial && record.serial.source !== "devd"),
  );
}

function rememberedHttpBaseUrl(record: DeviceRecord): string | null {
  return (
    record.target.rememberedChannels?.http?.baseUrl ??
    ((record.target.transport ?? "http") === "http"
      ? record.target.baseUrl
      : null)
  );
}

function rememberedHttpBaseUrls(record: DeviceRecord): string[] {
  const candidates = [
    record.target.rememberedChannels?.http?.baseUrl,
    record.target.rememberedChannels?.http?.fallbackBaseUrl,
    (record.target.transport ?? "http") === "http"
      ? record.target.baseUrl
      : null,
  ];
  const result: string[] = [];
  for (const candidate of candidates) {
    if (!candidate) continue;
    const baseUrl = normalizeBaseUrl(candidate);
    if (baseUrl && !result.includes(baseUrl)) result.push(baseUrl);
  }
  return result;
}

function rememberedDevdBaseUrl(record: DeviceRecord): string | null {
  return (
    record.target.rememberedChannels?.devd?.baseUrl ??
    (record.serial?.source === "devd"
      ? (record.serial.baseUrl ?? null)
      : record.target.transport === "devd"
        ? record.target.baseUrl
        : null)
  );
}

function sortTransportsByPreference<T extends DeviceChannelTransport>(
  transports: T[],
  preferred: DeviceChannelTransport,
): T[] {
  const order: DeviceChannelTransport[] = [preferred, "devd", "http", "serial"];
  return [...transports].sort(
    (left, right) => order.indexOf(left) - order.indexOf(right),
  );
}

function transportBadgeClass(
  transport: DeviceChannelTransport,
): "http" | "serial" | "devd" {
  if (transport === "http") return "http";
  if (transport === "devd") return "devd";
  return "serial";
}

function channelBadgeLabel(transport: DeviceChannelTransport): string {
  if (transport === "http") return "WiFi";
  if (transport === "devd") return "devd";
  return "USB";
}

function channelActionLabel(transport: DeviceChannelTransport): string {
  if (transport === "http") return "WiFi";
  if (transport === "devd") return "USB";
  return "USB";
}

function channelDiscoverActionText(transport: DeviceChannelTransport): string {
  return transport === "http" ? "Add WiFi" : "Bind USB";
}

function channelDiscoverBusyText(transport: DeviceChannelTransport): string {
  return transport === "http" ? "Adding" : "Binding";
}

function channelUseText(
  transport: DeviceChannelTransport,
  active = false,
): string {
  return `${active ? "Using" : "Use"} ${channelActionLabel(transport)}`;
}

function channelOpenText(transport: DeviceChannelTransport): string {
  return `Open with ${channelActionLabel(transport)}`;
}

function devdLogicalDeviceId(device: DevdDevice): string | null {
  return (
    device.binding?.logical_device_id ?? device.identity?.device_id ?? null
  );
}

function bindTargetLabel(record: DeviceRecord): string {
  return `${record.target.alias} (${record.target.deviceId})`;
}

function discoveryUsbChannel(device: DevdDevice): DevdDevice | null {
  return device.transport === "lan" ? null : device;
}

function discoveryHttpChannel(device: DevdDevice): DevdDevice | null {
  if (device.transport === "lan") return device;
  if (!device.lan_address) return null;
  return {
    ...device,
    transport: "lan",
    port_path: null,
    lan_address: device.lan_address,
    connection:
      (device.lan_conflict_addresses?.length ?? 0) > 0 ? "error" : "connected",
  };
}

export function buildDiscoveredLogicalDevices(
  devices: DevdDevice[],
  records: DeviceRecord[],
): DiscoveredLogicalDevice[] {
  const grouped = new Map<string, DiscoveredLogicalDevice>();
  for (const device of devices) {
    const deviceId = devdLogicalDeviceId(device);
    const key = deviceId ?? `pending:${device.id}`;
    const existing = grouped.get(key);
    const nextChannels = {
      ...(existing?.channels ?? {}),
    } as DiscoveredLogicalDevice["channels"];
    const usbChannel = discoveryUsbChannel(device);
    const httpChannel = discoveryHttpChannel(device);
    if (usbChannel) nextChannels.devd = usbChannel;
    if (httpChannel) nextChannels.http = httpChannel;
    const primaryDevice = nextChannels.devd ?? nextChannels.http ?? device;
    const existingRecord = deviceId
      ? (records.find((record) => record.target.deviceId === deviceId) ??
        existing?.existingRecord ??
        null)
      : null;
    const availableTransports = sortTransportsByPreference(
      (["devd", "http"] as const).filter(
        (candidate) => nextChannels[candidate],
      ),
      existingRecord
        ? preferredRecordTransport(existingRecord)
        : nextChannels.devd
          ? "devd"
          : "http",
    );
    const endpoints = availableTransports
      .map((candidate) => nextChannels[candidate])
      .filter((candidate): candidate is DevdDevice => Boolean(candidate))
      .map((candidate) => devdDeviceEndpoint(candidate));
    grouped.set(key, {
      key,
      deviceId,
      displayName:
        existingRecord?.target.alias ?? deviceId ?? primaryDevice.display_name,
      endpoint: endpoints.join(" / "),
      existingRecord,
      pendingCompanionCandidate:
        nextChannels.devd?.companion_lan_candidate ?? null,
      channels: nextChannels,
      availableTransports,
      connectionLabel: availableTransports
        .map(
          (candidate) =>
            `${channelBadgeLabel(candidate)} ${nextChannels[candidate]?.connection ?? "pending"}`,
        )
        .join(" / "),
      firmwareLabel:
        primaryDevice.identity?.firmware.build_id ?? "identity pending",
      logLabel: availableTransports
        .map(
          (candidate) =>
            `${channelBadgeLabel(candidate)} ${nextChannels[candidate]?.log_decode.status ?? "pending"}`,
        )
        .join(" / "),
    });
  }
  return Array.from(grouped.values());
}

export function buildFleetEntries(
  records: DeviceRecord[],
  devdDevices: DevdDevice[],
  devdTarget: string | null,
): FleetDeviceEntry[] {
  const entries = new Map<string, FleetDeviceEntry>();
  for (const record of records) {
    if (devdTarget && record.target.temporary) continue;
    entries.set(record.target.deviceId, {
      key: record.target.deviceId,
      record,
      saved: !record.target.temporary,
    });
  }
  if (!devdTarget) return Array.from(entries.values());
  const devdBaseUrl = normalizeBaseUrl(devdTarget);
  for (const discovered of buildDiscoveredLogicalDevices(devdDevices, records)) {
    if (!discovered.deviceId) continue;
    const current = entries.get(discovered.deviceId);
    const mergedRecord = buildFleetEntryRecord(
      discovered,
      current?.record ?? discovered.existingRecord,
      devdBaseUrl,
    );
    if (!mergedRecord) continue;
    entries.set(discovered.deviceId, {
      key: discovered.deviceId,
      record: mergedRecord,
      saved:
        current?.saved ??
        Boolean(discovered.existingRecord && !discovered.existingRecord.target.temporary),
    });
  }
  return Array.from(entries.values());
}

function buildFleetEntryRecord(
  discovered: DiscoveredLogicalDevice,
  existingRecord: DeviceRecord | null | undefined,
  devdBaseUrl: string,
): DeviceRecord | null {
  const deviceId = discovered.deviceId;
  if (!deviceId) return null;
  const httpDevice = discovered.channels.http ?? null;
  const devdDevice = discovered.channels.devd ?? null;
  const identity =
    httpDevice?.identity ??
    devdDevice?.identity ??
    existingRecord?.identity ??
    null;
  const companion = httpDevice?.binding?.lan_companion ?? null;
  const companionBaseUrl = companion
    ? normalizeBaseUrl(companion.mdns_host)
    : null;
  const companionFallbackAddress = httpDevice?.lan_address ?? companion?.ip;
  const companionFallbackBaseUrl = companionFallbackAddress && companion
    ? normalizeBaseUrl(`${companionFallbackAddress}:${companion.port}`)
    : null;
  const httpBaseUrl =
    companionBaseUrl ?? devdLanBaseUrl(httpDevice, identity);
  const devdRecordId =
    devdDevice?.id ??
    httpDevice?.id ??
    existingRecord?.target.rememberedChannels?.devd?.devdDeviceId ??
    deviceId;
  const target = {
    deviceId,
    baseUrl: devdBaseUrl,
    alias:
      existingRecord?.target.alias ??
      identity?.hostname ??
      discovered.displayName,
    location: existingRecord?.target.location ?? "devd records",
    addedAt: existingRecord?.target.addedAt ?? new Date().toISOString(),
    transport: "devd" as const,
    preferredTransport: "devd" as const,
    rememberedChannels: {
      ...existingRecord?.target.rememberedChannels,
      ...(httpDevice
        ? {
            http: {
              baseUrl:
                httpBaseUrl ??
                existingRecord?.target.rememberedChannels?.http?.baseUrl ??
                existingRecord?.target.baseUrl ??
                "",
              seenAt: new Date().toISOString(),
              source: "devd_discovery" as const,
              mdnsHost:
                companion?.mdns_host ??
                existingRecord?.target.rememberedChannels?.http?.mdnsHost,
              fallbackBaseUrl:
                companionFallbackBaseUrl ??
                existingRecord?.target.rememberedChannels?.http
                  ?.fallbackBaseUrl,
            },
          }
        : {}),
      devd: {
        baseUrl: devdBaseUrl,
        devdDeviceId: devdRecordId,
        seenAt: new Date().toISOString(),
        transport: devdDevice
          ? (devdDevice.transport === "mock" ? "mock" : "usb")
          : "lan",
      },
    },
  } satisfies DeviceRecord["target"];
  const connected =
    httpDevice?.connection === "connected" ||
    devdDevice?.connection === "connected";
  const connecting =
    httpDevice?.connection === "busy" || devdDevice?.connection === "busy";
  const errored =
    httpDevice?.connection === "error" || devdDevice?.connection === "error";
  const preserveTransportFailure =
    existingRecord?.target.temporary === true &&
    hasTransportFailure(existingRecord);
  const recoveredTransportFailure =
    !preserveTransportFailure &&
    connected &&
    hasTransportFailure(existingRecord);
  const recoveredActionError =
    !preserveTransportFailure &&
    connected &&
    existingRecord?.target.transport === "http" &&
    Boolean(existingRecord.error) &&
    existingRecord.errorSource !== "read" &&
    !hasTransportFailure(existingRecord);
  const currentStatus = httpDevice?.status ?? devdDevice?.status ?? null;
  return {
    target,
    identity,
    network: identity?.network ?? existingRecord?.network ?? null,
    settings: existingRecord?.settings ?? null,
    status:
      currentStatus ??
      (recoveredTransportFailure ? null : existingRecord?.status ?? null),
    connectionState: preserveTransportFailure
      ? existingRecord!.connectionState
      : connected
        ? "online"
        : connecting
          ? "connecting"
          : errored
            ? "error"
            : (existingRecord?.connectionState ?? "offline"),
    streamState: preserveTransportFailure
      ? existingRecord!.streamState
      : recoveredTransportFailure
        ? currentStatus
          ? "polling"
          : "idle"
        : recoveredActionError
          ? currentStatus
            ? "polling"
            : "idle"
        : (existingRecord?.streamState ?? "idle"),
    error: recoveredTransportFailure ? null : existingRecord?.error ?? null,
    errorSource: recoveredTransportFailure
      ? undefined
      : existingRecord?.errorSource,
    commandError: existingRecord?.commandError,
    lastUpdated: new Date().toISOString(),
    serial:
      existingRecord?.serial ??
      (devdDevice
        ? {
            connected: devdDevice.connection === "connected",
            source: "devd" as const,
            baseUrl: devdBaseUrl,
            protocol:
              devdDevice.identity?.firmware.protocol ??
              existingRecord?.target.serialProtocol ??
              "mains-aegis.cdc.v1",
            logs: [],
            trace: [],
          }
        : undefined),
  };
}

function devdDeviceEndpoint(device: DevdDevice): string {
  if (device.transport === "lan")
    return device.lan_address ?? device.display_name;
  return device.port_path ?? device.display_name;
}

function devdLanBaseUrl(
  device: DevdDevice | null,
  identity: DeviceRecord["identity"],
): string | null {
  const candidate =
    device?.lan_address?.trim() ||
    identity?.network.ipv4?.trim() ||
    identity?.hostname_fqdn?.trim() ||
    identity?.hostname?.trim() ||
    "";
  return candidate ? normalizeBaseUrl(candidate) : null;
}

function devdDeviceTransportLabel(device: DevdDevice): string {
  if (device.transport === "lan") return "LAN";
  if (device.transport === "mock") return "Mock";
  return "USB CDC";
}

function isConnectableDevdDevice(device: DevdDevice): boolean {
  if (device.transport === "mock") return true;
  if (device.transport === "native_serial") return Boolean(device.port_path);
  if (device.transport !== "lan") return false;
  return (
    isMainsAegisLanDevice(device) &&
    (device.lan_conflict_addresses?.length ?? 0) === 0
  );
}

function isMainsAegisLanDevice(device: DevdDevice): boolean {
  return (
    device.transport === "lan" &&
    device.identity?.firmware.protocol === "mains-aegis.cdc.v1"
  );
}

export function ConnectPageHeading({ description }: { description: string }) {
  return (
    <div className="section-heading">
      <h1>Add device</h1>
      <p>{description}</p>
    </div>
  );
}

export function ConnectPage({
  initialDevdTarget,
  hostedHttpServiceApp = isHostedHttpServiceApp(),
  sharedDevdDiscovery,
}: {
  initialDevdTarget?: string;
  hostedHttpServiceApp?: boolean;
  sharedDevdDiscovery?: SharedDevdDiscovery;
}) {
  const {
    records,
    demoSeed,
    addDevice,
    addDevdDevice,
    confirmDevdCompanionLan,
    dismissDevdCompanionLan,
    connectUsbSerialDevice,
    connectKnownDeviceChannel,
    rememberDiscoveredChannels,
    attachMockUsbSerialDevice,
    disconnectUsbSerialDevice,
    removeDevice,
    refreshDevice,
    resetDemo,
  } = useDeviceRegistry();
  const demoMode = demoSeed !== null;
  const queryBindLogicalDeviceId =
    new URLSearchParams(window.location.search)
      .get("mock_bind_logical_device_id")
      ?.trim() || "";
  const queryMockBrowserCapability =
    new URLSearchParams(window.location.search)
      .get("mock_browser_capability")
      ?.trim() || "";
  const [target, setTarget] = useState("");
  const [alias, setAlias] = useState("");
  const [location, setLocation] = useState("");
  const [cidr, setCidr] = useState("");
  const [usbAlias, setUsbAlias] = useState("");
  const [usbLocation, setUsbLocation] = useState("");
  const [fallbackDevdTarget] = useState(() =>
    demoMode
      ? (initialDevdTarget ?? "mock:devd")
      : (initialDevdTarget ??
        (hostedHttpServiceApp ? "same-origin" : envDevdTarget)),
  );
  const [message, setMessage] = useState<UiFeedback | null>(null);
  const [usbMessage, setUsbMessage] = useState<UiFeedback | null>(null);
  const [devdMessage, setDevdMessage] = useState<UiFeedback | null>(null);
  const [usbFirmwareOverridePending, setUsbFirmwareOverridePending] =
    useState(false);
  const [devdFirmwareOverrideMessage, setDevdFirmwareOverrideMessage] =
    useState<UiFeedback | null>(null);
  const [devdFirmwareOverrideDeviceId, setDevdFirmwareOverrideDeviceId] =
    useState<string | null>(null);
  const [devdConnectingDeviceId, setDevdConnectingDeviceId] = useState<
    string | null
  >(null);
  const [savedDeviceMessage, setSavedDeviceMessage] =
    useState<UiFeedback | null>(null);
  const [savedDeviceSwitchTarget, setSavedDeviceSwitchTarget] = useState<{
    deviceId: string;
    transport: DeviceChannelTransport;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [usbBusy, setUsbBusy] = useState(false);
  const [devdBusy, setDevdBusy] = useState(false);
  const [companionConfirmingDeviceId, setCompanionConfirmingDeviceId] =
    useState<string | null>(null);
  const [companionDismissingDeviceId, setCompanionDismissingDeviceId] =
    useState<string | null>(null);
  const [devdBindTargets, setDevdBindTargets] = useState<Record<string, string>>(
    () => {
      const initialTargets: Record<string, string> = {};
      if (queryBindLogicalDeviceId) {
        initialTargets["mock-devd-usb-pending"] = queryBindLogicalDeviceId;
      }
      return initialTargets;
    },
  );
  const [scanState, setScanState] = useState<ScanState>({
    status: "idle",
    cidr: "",
    message: null,
    candidates: [],
  });
  const serialSupported = isWebSerialSupported();
  const devdDiscoveryOnly = hostedHttpServiceApp;
  const devdTarget = sharedDevdDiscovery?.devdTarget ?? fallbackDevdTarget;
  const devdDevices = sharedDevdDiscovery?.devdDevices ?? [];
  const devdStatus = sharedDevdDiscovery?.status ?? "checking";
  const devdLastUpdated = sharedDevdDiscovery?.lastUpdated ?? null;
  const runtimeMode = resolveConnectRuntimeMode({
    hostedHttpServiceApp,
    devdTarget,
  });
  const browserLanCapability = useMemo(() => {
    if (demoMode && queryMockBrowserCapability === "supported") {
      return detectBrowserLanCapability({
        isSecureContext: true,
        userAgent:
          "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36",
      });
    }
    if (demoMode && queryMockBrowserCapability === "unsupported") {
      return detectBrowserLanCapability({
        isSecureContext: false,
        userAgent:
          "Mozilla/5.0 AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
      });
    }
    return detectBrowserLanCapability();
  }, [demoMode, queryMockBrowserCapability]);
  const publicStaticBuild =
    isPublicStaticApp() || envRuntimeMode === "public_static";
  const showLanScanPanel = publicStaticBuild;
  const bindTargetOptions = useMemo<BindTargetOption[]>(
    () =>
      [...records]
        .sort((left, right) =>
          left.target.alias.localeCompare(right.target.alias),
        )
        .map((record) => ({
          deviceId: record.target.deviceId,
          label: bindTargetLabel(record),
        })),
    [records],
  );

  const refreshDevdDiscovery = useCallback(
    async (options: { clearMessage?: boolean } = {}) => {
      if (options.clearMessage) {
        setDevdMessage(null);
        setDevdFirmwareOverrideMessage(null);
        setDevdFirmwareOverrideDeviceId(null);
      }
      if (!sharedDevdDiscovery) return;
      try {
        await sharedDevdDiscovery.refresh();
        if (!options.clearMessage) setDevdMessage(null);
      } catch (error) {
        setDevdMessage(errorFeedback(toErrorEnvelope(error)));
      }
    },
    [sharedDevdDiscovery],
  );

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    if (publicStaticBuild && !browserLanCapability.supported) {
      setMessage(
        errorFeedback({
          code: "browser_lan_capability_required",
          message:
            browserLanCapability.reason ??
            "Use Chrome 142+ in a secure context for LAN access.",
          retryable: false,
          details: { runtimeMode },
        }),
      );
      return;
    }
    setBusy(true);
    setMessage(null);
    const rememberedHttpChannel = resolveManualHttpRememberedChannel(target);
    const result = await addDevice({
      target,
      alias,
      location,
      ...rememberedHttpChannel,
    });
    setBusy(false);
    if (result.ok) {
      setTarget("");
      setAlias("");
      setLocation("");
      setMessage(successFeedback(`Connected ${result.record.target.alias}`));
    } else {
      setMessage(errorFeedback(result.error));
    }
  }

  async function onScanSubmit(event: FormEvent) {
    event.preventDefault();
    if (!browserLanCapability.supported) {
      setScanState((current) => ({
        ...current,
        message: errorFeedback({
          code: "browser_lan_capability_required",
          message:
            browserLanCapability.reason ??
            "Use Chrome 142+ in a secure context for LAN access.",
          retryable: false,
          details: { runtimeMode },
        }),
      }));
      return;
    }
    let expanded;
    try {
      expanded = expandIpv4Cidr(cidr);
    } catch (error) {
      setScanState((current) => ({
        ...current,
        message: errorFeedback({
          code: "invalid_cidr",
          message:
            error instanceof Error
              ? error.message
              : "Use a valid IPv4 CIDR that expands to 2-256 hosts.",
          retryable: false,
          details: { cidr },
        }),
      }));
      return;
    }
    setScanState({
      status: "scanning",
      cidr: expanded.normalized,
      message: null,
      candidates: [],
    });
    const limit = 8;
    const timeoutMs = 800;
    const hosts = [...expanded.hosts];
    const candidates = new Map<string, ScanCandidate>();
    const worker = async () => {
      while (hosts.length > 0) {
        const nextHost = hosts.shift();
        if (!nextHost) return;
        const fallbackBaseUrl = normalizeBaseUrl(nextHost);
        try {
          const identity = await getIdentity(fallbackBaseUrl, undefined, {
            timeoutMs,
          });
          if (!isLanIdentityCandidate(identity)) {
            continue;
          }
          const mdnsHost =
            identity.hostname_fqdn?.trim() || identity.hostname?.trim() || null;
          const mdnsBaseUrl = mdnsHost
            ? normalizeBaseUrl(mdnsHost)
            : fallbackBaseUrl;
          const existingRecord =
            records.find(
              (record) => record.target.deviceId === identity.device_id,
            ) ?? null;
          candidates.set(identity.device_id, {
            key: identity.device_id,
            deviceId: identity.device_id,
            alias: existingRecord?.target.alias ?? identity.hostname,
            endpoints: [mdnsBaseUrl, fallbackBaseUrl].filter(
              (value, index, array): value is string =>
                Boolean(value) && array.indexOf(value) === index,
            ),
            baseUrl: mdnsBaseUrl,
            mdnsBaseUrl,
            mdnsHost,
            fallbackBaseUrl:
              fallbackBaseUrl !== mdnsBaseUrl ? fallbackBaseUrl : null,
            identity,
            existingRecord,
          });
        } catch {
          continue;
        }
      }
    };
    await Promise.all(
      Array.from({ length: Math.min(limit, expanded.hosts.length) }, () =>
        worker(),
      ),
    );
    const nextCandidates = Array.from(candidates.values()).sort((left, right) =>
      left.alias.localeCompare(right.alias),
    );
    setScanState({
      status: "done",
      cidr: expanded.normalized,
      message:
        nextCandidates.length > 0
          ? successFeedback(
              `Found ${nextCandidates.length} device${nextCandidates.length === 1 ? "" : "s"} in ${expanded.normalized}`,
            )
          : errorFeedback({
              code: "scan_empty",
              message: `No Mains Aegis device answered in ${expanded.normalized}.`,
              retryable: true,
              details: { cidr: expanded.normalized },
            }),
      candidates: nextCandidates,
    });
  }

  async function onScanCandidateAdd(candidate: ScanCandidate) {
    setBusy(true);
    setMessage(null);
    const result = await addDevice({
      target: candidate.fallbackBaseUrl ?? candidate.baseUrl,
      alias: candidate.existingRecord?.target.alias ?? candidate.alias,
      location: candidate.existingRecord?.target.location ?? location,
      rememberedHttpBaseUrl: candidate.mdnsBaseUrl,
      rememberedHttpMdnsHost: candidate.mdnsHost ?? undefined,
      rememberedHttpFallbackBaseUrl: candidate.fallbackBaseUrl ?? undefined,
    });
    setBusy(false);
    if (result.ok) {
      setMessage(successFeedback(`Connected ${result.record.target.alias}`));
      navigate(deviceHref(result.record.target.deviceId, "settings"));
      return;
    }
    setMessage(errorFeedback(result.error));
  }

  async function onUsbConnect(ignoreFirmwareMismatch = false) {
    setUsbBusy(true);
    setUsbMessage(null);
    const result = await connectUsbSerialDevice({
      alias: usbAlias,
      location: usbLocation,
      ignoreFirmwareMismatch,
    });
    setUsbBusy(false);
    if (result.ok) {
      setUsbFirmwareOverridePending(false);
      setUsbAlias("");
      setUsbLocation("");
      setUsbMessage(
        successFeedback(`USB connected ${result.record.target.alias}`),
      );
      navigate(deviceHref(result.record.target.deviceId, "settings"));
    } else {
      setUsbFirmwareOverridePending(
        result.error?.code === "firmware_artifact_mismatch",
      );
      setUsbMessage(errorFeedback(result.error));
    }
  }

  async function onDiscoveredDeviceAction(
    device: DevdDevice,
    ignoreFirmwareMismatch = false,
    bindLogicalDeviceId?: string,
  ) {
    if (!isConnectableDevdDevice(device)) {
      setDevdMessage(
        errorFeedback({
          code: "device_not_connectable",
          message: "This devd device is not ready for Web connection yet",
          retryable: true,
          details: { device },
        }),
      );
      return;
    }
    const devdBaseUrl = normalizeBaseUrl(devdTarget);
    const transport: Extract<DeviceChannelTransport, "http" | "devd"> =
      device.transport === "lan" ? "http" : "devd";
    const discoveredRecord = devdLogicalDeviceId(device)
      ? (records.find(
          (record) => record.target.deviceId === devdLogicalDeviceId(device),
        ) ?? null)
      : null;
    const bindTargetRecord = bindLogicalDeviceId
      ? (records.find(
          (record) => record.target.deviceId === bindLogicalDeviceId,
        ) ?? null)
      : null;
    const existingRecord = discoveredRecord ?? bindTargetRecord;
    const bindOnly =
      transport === "devd" && !device.binding && Boolean(bindTargetRecord);
    setDevdConnectingDeviceId(device.id);
    setDevdBusy(true);
    setDevdMessage(null);
    setDevdFirmwareOverrideMessage(null);
    setDevdFirmwareOverrideDeviceId(null);
    try {
      if (!discoveredRecord && transport === "devd" && !device.binding) {
        await bindDevdDevice(
          device.id,
          { logicalDeviceId: bindTargetRecord?.target.deviceId },
          devdBaseUrl,
        );
        if (bindOnly) {
          await refreshDevdDiscovery({ clearMessage: true }).catch(
            () => undefined,
          );
          setDevdBusy(false);
          setDevdConnectingDeviceId(null);
          setDevdBindTargets((current) => {
            const next = { ...current };
            delete next[device.id];
            return next;
          });
          setDevdMessage(
            successFeedback(
              `Bound USB for ${bindTargetRecord?.target.alias ?? bindTargetRecord?.target.deviceId ?? "saved device"}`,
            ),
          );
          return;
        }
      }
    } catch (error) {
      setDevdBusy(false);
      setDevdConnectingDeviceId(null);
      setDevdMessage(errorFeedback(toErrorEnvelope(error)));
      return;
    }
    const result = existingRecord
      ? await connectKnownDeviceChannel(
          existingRecord.target.deviceId,
          transport,
          { ignoreFirmwareMismatch },
        )
      : await addDevdDevice({
          target: devdTarget,
          devdDeviceId: device.id,
          ignoreFirmwareMismatch,
        });
    setDevdBusy(false);
    setDevdConnectingDeviceId(null);
    if (result.ok) {
      setDevdFirmwareOverrideDeviceId(null);
      setDevdFirmwareOverrideMessage(null);
      setDevdBindTargets((current) => {
        const next = { ...current };
        delete next[device.id];
        return next;
      });
      const actionLabel = existingRecord
        ? `Using ${channelActionLabel(transport)}`
        : transport === "devd"
          ? "Bound USB"
          : "Added WiFi";
      setDevdMessage(
        successFeedback(`${actionLabel} for ${result.record.target.alias}`),
      );
      navigate(deviceHref(result.record.target.deviceId, "settings"));
      void refreshDevdDiscovery();
    } else {
      const feedback = errorFeedback(result.error);
      const firmwareMismatch =
        result.error?.code === "firmware_artifact_mismatch";
      setDevdFirmwareOverrideDeviceId(firmwareMismatch ? device.id : null);
      setDevdFirmwareOverrideMessage(firmwareMismatch ? feedback : null);
      setDevdMessage(feedback);
    }
  }

  async function onConfirmCompanionLan(device: DevdDevice) {
    setCompanionConfirmingDeviceId(device.id);
    setDevdMessage(null);
    const result = await confirmDevdCompanionLan(
      device.id,
      normalizeBaseUrl(devdTarget),
    );
    setCompanionConfirmingDeviceId(null);
    if (result.ok) {
      setDevdMessage(
        successFeedback(
          `Added LAN companion for ${result.record.target.alias}`,
        ),
      );
      void refreshDevdDiscovery({ clearMessage: false });
      return;
    }
    setDevdMessage(errorFeedback(result.error));
  }

  async function onDismissCompanionLan(device: DevdDevice) {
    setCompanionDismissingDeviceId(device.id);
    setDevdMessage(null);
    const result = await dismissDevdCompanionLan(
      device.id,
      normalizeBaseUrl(devdTarget),
    );
    setCompanionDismissingDeviceId(null);
    if (result.ok) {
      setDevdMessage(
        successFeedback(`Ignored LAN companion prompt for ${device.display_name}`),
      );
      void refreshDevdDiscovery({ clearMessage: false });
      return;
    }
    setDevdMessage(errorFeedback(result.error));
  }

  async function onSavedDeviceChannelSwitch(
    record: DeviceRecord,
    transport: DeviceChannelTransport,
  ) {
    setSavedDeviceSwitchTarget({ deviceId: record.target.deviceId, transport });
    setSavedDeviceMessage(null);
    try {
      const result = await connectKnownDeviceChannel(
        record.target.deviceId,
        transport,
      );
      if (result.ok) {
        setSavedDeviceMessage(
          successFeedback(
            `Switched ${result.record.target.alias} to ${channelBadgeLabel(transport)}`,
          ),
        );
        navigate(deviceDefaultHref(result.record));
        if (transport === "devd" || transport === "http")
          void refreshDevdDiscovery();
        return;
      }
      setSavedDeviceMessage(errorFeedback(result.error));
    } catch (error) {
      setSavedDeviceMessage(errorFeedback(toErrorEnvelope(error)));
    } finally {
      setSavedDeviceSwitchTarget(null);
    }
  }

  function onMockUsbConnect() {
    const result = attachMockUsbSerialDevice();
    if (result.ok) {
      setUsbMessage(
        successFeedback(`USB demo attached ${result.record.target.alias}`),
      );
      navigate(deviceHref(result.record.target.deviceId, "settings"));
    }
  }

  const discoveredLogicalDevices = useMemo(
    () => buildDiscoveredLogicalDevices(devdDevices, records),
    [devdDevices, records],
  );
  const visibleDevdMessage = devdFirmwareOverrideMessage ?? devdMessage;
  const devdSummary =
    devdStatus === "checking"
      ? "Loading device records"
      : devdStatus === "available"
        ? `${discoveredLogicalDevices.length} devices across ${devdDevices.length} reported channels`
        : "Not reachable";
  const showLanFallback =
    !devdDiscoveryOnly &&
    (runtimeMode === "standalone_no_devd" ||
      runtimeMode === "public_static" ||
      devdStatus === "unavailable");
  const showFallbackConnectPanels = !devdDiscoveryOnly;
  const devdLastUpdatedLabel = devdLastUpdated
    ? timeAgo(devdLastUpdated)
    : "not yet";

  return (
    <section className="page-flow connect-wide">
      <ConnectPageHeading
        description={
          devdDiscoveryOnly
            ? "Use this page to add hardware from current mains-aegis-devd device records. USB devices attach through devd, while LAN devices connect directly to the hardware HTTP API."
            : runtimeMode === "public_static"
              ? "This GitHub Pages build connects to LAN devices directly from the browser. Use Chrome 142+ for manual targets or CIDR scans; hosted devd discovery is not assumed here."
              : "Use this page to add a new device, bind a new USB port, or add a LAN endpoint. When mains-aegis-devd is reachable, current USB CDC and LAN device records appear here automatically."
        }
      />
      {devdTarget ? (
      <section
        className="devd-discovery-panel"
        data-evidence-target="devd-discovery"
      >
        <header className="devd-discovery-header">
          <div>
            <span className="eyebrow">mains-aegis-devd</span>
            <h3>
              <Server size={19} /> mains-aegis-devd device records
            </h3>
            <p>
          {devdStatus === "unavailable"
                ? devdDiscoveryOnly
                  ? "This hosted UI depends on mains-aegis-devd device records. Restart or reconnect devd to continue."
                  : "Manual LAN entry is available below because devd cannot be reached."
                : devdDiscoveryOnly
                  ? "USB devices attach through devd. LAN devices are reported by devd, then connected directly to the hardware HTTP API."
                  : publicStaticBuild
                    ? "This public static build can also point at an explicit devd URL, but LAN direct entry remains available below as the primary browser path."
                    : "Current USB and LAN device records refresh automatically while this page is open."}
            </p>
          </div>
          <div className="devd-discovery-status">
            <span
              className={`transport-badge ${devdStatus === "available" ? "devd" : devdStatus === "unavailable" ? "offline" : "adapter"}`}
            >
              {devdSummary}
            </span>
            <button
              className="icon-button"
              type="button"
              aria-label="Refresh devd device list"
              title="Refresh devd device list"
              onClick={() => void refreshDevdDiscovery({ clearMessage: true })}
              disabled={devdBusy}
            >
              <RefreshCw size={16} />
            </button>
          </div>
        </header>

        <div className="devd-device-list" aria-live="polite">
          {devdStatus === "checking" && devdDevices.length === 0 ? (
            <div className="devd-empty-state">
              <Loader2 size={18} className="spin-icon" />
              <strong>Loading devd device records</strong>
              <span>Reading current USB and LAN records from mains-aegis-devd.</span>
            </div>
          ) : null}
          {devdStatus === "available" && devdDevices.length === 0 ? (
            <div className="devd-empty-state">
              <Radio size={18} />
              <strong>No device records yet</strong>
              <span>
                devd is reachable, but it is not reporting any USB or LAN
                device records right now.
              </span>
            </div>
          ) : null}
          {discoveredLogicalDevices.map((device) => {
            const existingRecord = device.existingRecord;
            const activeTransport = existingRecord
              ? activeRecordTransport(existingRecord)
              : null;
            const defaultTransport = device.availableTransports[0];
            const primaryChannel = device.channels[defaultTransport];
            const showOverride =
              devdFirmwareOverrideDeviceId === primaryChannel?.id;
            const pendingCompanion = device.pendingCompanionCandidate;
            const devdChannel = device.channels.devd ?? null;
            const showPendingCompanion =
              Boolean(devdChannel) &&
              Boolean(pendingCompanion) &&
              !devdChannel?.binding?.lan_companion;
            const isConnectingDevice = primaryChannel
              ? devdConnectingDeviceId === primaryChannel.id
              : false;
            const needsBindTargetSelection =
              !existingRecord &&
              primaryChannel?.transport === "native_serial" &&
              !primaryChannel.binding?.logical_device_id &&
              device.deviceId === null &&
              bindTargetOptions.length > 0;
            const selectedBindTargetId =
              primaryChannel && needsBindTargetSelection
                ? (devdBindTargets[primaryChannel.id] ??
                  queryBindLogicalDeviceId ??
                  (bindTargetOptions.length === 1
                    ? (bindTargetOptions[0]?.deviceId ?? "")
                    : ""))
                : "";
            const alternateTransportOptions = device.availableTransports
              .filter((transport) => transport !== defaultTransport)
              .map((transport) => {
                const channel = device.channels[transport];
                const isCurrent =
                  activeTransport === transport &&
                  existingRecord?.connectionState === "online";
                return {
                  value: transport,
                  label: existingRecord
                    ? channelUseText(transport, isCurrent)
                    : channelDiscoverActionText(transport),
                  disabled:
                    !channel ||
                    !isConnectableDevdDevice(channel) ||
                    devdBusy ||
                    isCurrent,
                };
              });
            const openTransportOptions = [
              {
                value: defaultTransport as Extract<
                  DeviceChannelTransport,
                  "http" | "devd"
                >,
                label: channelOpenText(defaultTransport),
                disabled:
                  !primaryChannel ||
                  !isConnectableDevdDevice(primaryChannel) ||
                  devdBusy,
              },
              ...alternateTransportOptions.map((option) => ({
                ...option,
                label: channelOpenText(
                  option.value as Extract<DeviceChannelTransport, "http" | "devd">,
                ),
              })),
            ];
            return (
              <article
                className={`devd-device-card ${primaryChannel && isConnectableDevdDevice(primaryChannel) ? "" : "is-muted"}`}
                key={device.key}
              >
                <div className="devd-device-main">
                  <span
                    className={`transport-badge ${transportBadgeClass(defaultTransport)}`}
                  >
                    {channelBadgeLabel(defaultTransport)}
                  </span>
                  <div>
                    <h4>{device.displayName}</h4>
                    <p>{device.endpoint}</p>
                  </div>
                </div>
                <dl className="devd-device-meta">
                  <div>
                    <dt>Connection</dt>
                    <dd>{device.connectionLabel}</dd>
                  </div>
                  <div>
                    <dt>Firmware</dt>
                    <dd>{device.firmwareLabel}</dd>
                  </div>
                  <div>
                    <dt>Logs</dt>
                    <dd>{device.logLabel}</dd>
                  </div>
                </dl>
                <div className="devd-device-actions">
                  {existingRecord && primaryChannel ? (
                    <DeviceOpenAction
                      navigateTo={() =>
                        navigate(deviceDefaultHref(existingRecord))
                      }
                      menuLabel={`Choose connection for ${device.displayName}`}
                      options={openTransportOptions}
                      onSelect={(transport) => {
                        const channel = device.channels[transport];
                        if (!channel) return;
                        void onDiscoveredDeviceAction(channel);
                      }}
                    />
                  ) : primaryChannel ? (
                    <>
                      {needsBindTargetSelection ? (
                        <label className="devd-bind-target-select">
                          <span>Bind to</span>
                          <select
                            aria-label={`Bind USB target for ${device.displayName}`}
                            value={selectedBindTargetId}
                            onChange={(event) =>
                              setDevdBindTargets((current) => ({
                                ...current,
                                [primaryChannel.id]: event.target.value,
                              }))
                            }
                          >
                            <option value="">Choose saved device</option>
                            {bindTargetOptions.map((option) => (
                              <option
                                key={option.deviceId}
                                value={option.deviceId}
                              >
                                {option.label}
                              </option>
                            ))}
                          </select>
                        </label>
                      ) : null}
                      <button
                        className="secondary-button small"
                        type="button"
                        disabled={
                          devdBusy ||
                          !isConnectableDevdDevice(primaryChannel) ||
                          (needsBindTargetSelection &&
                            selectedBindTargetId === "")
                        }
                        onClick={() =>
                          void onDiscoveredDeviceAction(
                            primaryChannel,
                            false,
                            selectedBindTargetId || undefined,
                          )
                        }
                      >
                        <ButtonLabel
                          busy={isConnectingDevice}
                          busyText={channelDiscoverBusyText(defaultTransport)}
                          text={channelDiscoverActionText(defaultTransport)}
                        />
                      </button>
                      {alternateTransportOptions.map((option) => {
                        const nextTransport = option.value as Extract<
                          DeviceChannelTransport,
                          "http" | "devd"
                        >;
                        const channel = device.channels[nextTransport];
                        if (!channel) return null;
                        return (
                          <button
                            key={option.value}
                            className="secondary-button small"
                            type="button"
                            disabled={option.disabled}
                            onClick={() =>
                              void onDiscoveredDeviceAction(channel)
                            }
                          >
                            {option.label}
                          </button>
                        );
                      })}
                    </>
                  ) : null}
                  {showOverride ? (
                    <button
                      className="secondary-button danger-action"
                      type="button"
                      disabled={devdBusy || !primaryChannel}
                      onClick={() =>
                        primaryChannel
                          ? void onDiscoveredDeviceAction(primaryChannel, true)
                          : undefined
                      }
                    >
                      Ignore warning
                    </button>
                  ) : null}
                </div>
                {showPendingCompanion && devdChannel ? (
                  <div className="inline-companion-callout">
                    <div className="inline-companion-signal" aria-hidden="true">
                      <Globe2 size={18} />
                    </div>
                    <div className="inline-companion-copy">
                      <div className="inline-companion-row">
                        <CompanionHelpBubble
                          mdnsHost={pendingCompanion?.mdns_host}
                          ip={pendingCompanion?.ip}
                          port={pendingCompanion?.port}
                        />
                        <span className="inline-companion-target">
                          <span className="inline-companion-target-label">
                            devd
                          </span>
                          <code>{pendingCompanion?.mdns_host}</code>
                        </span>
                        <span className="inline-companion-target">
                          <span className="inline-companion-target-label">
                            Web
                          </span>
                          <code>
                            http://{pendingCompanion?.ip}:{pendingCompanion?.port}
                          </code>
                        </span>
                      </div>
                    </div>
                    <div className="inline-companion-actions">
                      <button
                        className="secondary-button small"
                        type="button"
                        disabled={
                          devdBusy ||
                          companionDismissingDeviceId === devdChannel.id ||
                          companionConfirmingDeviceId === devdChannel.id
                        }
                        onClick={() => void onDismissCompanionLan(devdChannel)}
                      >
                        <ButtonLabel
                          busy={companionDismissingDeviceId === devdChannel.id}
                          busyText="Ignoring"
                          text="Not now"
                        />
                      </button>
                      <button
                        className="primary-button small"
                        type="button"
                        disabled={
                          devdBusy ||
                          companionConfirmingDeviceId === devdChannel.id ||
                          companionDismissingDeviceId === devdChannel.id
                        }
                        onClick={() => void onConfirmCompanionLan(devdChannel)}
                      >
                        <ButtonLabel
                          busy={companionConfirmingDeviceId === devdChannel.id}
                          busyText="Saving"
                          text="Bind LAN now"
                        />
                      </button>
                    </div>
                  </div>
                ) : null}
              </article>
            );
          })}
        </div>
        <footer className="devd-discovery-footer">
          <span>Last refresh: {devdLastUpdatedLabel}</span>
          <span>
            Events trigger refresh when the HTTP service supports
            `/api/v1/devices/events`; polling remains active.
          </span>
        </footer>
        {visibleDevdMessage?.tone === "error" ? (
          <ConnectionCallout
            id="devd-connect-message"
            message={visibleDevdMessage.message}
          />
        ) : null}
        {visibleDevdMessage?.tone === "success" ? (
          <FeedbackMessage feedback={visibleDevdMessage} />
        ) : null}
      </section>
      ) : null}

      {showFallbackConnectPanels ? (
        <div
          className="connect-grid secondary-connect-grid"
          data-evidence-target="usb-connect"
        >
          <section className="connect-panel usb-panel">
            <header className="connect-panel-header">
              <div>
                <h3>
                  <Usb size={18} /> Web Serial
                </h3>
                <p>
                  {serialSupported
                    ? "Browser-local fallback for USB CDC devices when devd is not available"
                    : "Web Serial unavailable in this browser"}
                </p>
              </div>
              <span
                className={`transport-badge ${serialSupported ? "serial" : "offline"}`}
              >
                {serialSupported ? "ready" : "unsupported"}
              </span>
            </header>
            <div className="connect-form compact">
              <label>
                Alias
                <input
                  name="usb-alias"
                  value={usbAlias}
                  onChange={(event) => setUsbAlias(event.target.value)}
                  placeholder="Lab bench USB"
                />
              </label>
              <label>
                Location
                <input
                  name="usb-location"
                  value={usbLocation}
                  onChange={(event) => setUsbLocation(event.target.value)}
                  placeholder="Bench 1"
                />
              </label>
              <div className="form-actions with-callout">
                <button
                  className="primary-button"
                  type="button"
                  disabled={usbBusy || !serialSupported}
                  onClick={() => void onUsbConnect()}
                  aria-describedby={
                    usbMessage?.tone === "error"
                      ? "usb-connect-message"
                      : undefined
                  }
                >
                  <ButtonLabel
                    icon={Usb}
                    busy={usbBusy}
                    busyText="Connecting"
                    text="Connect Web Serial"
                  />
                </button>
                {usbMessage?.tone === "error" ? (
                  <ConnectionCallout
                    id="usb-connect-message"
                    message={usbMessage.message}
                  />
                ) : null}
                {demoMode ? (
                  <button
                    className="secondary-button"
                    type="button"
                    onClick={onMockUsbConnect}
                  >
                    <Terminal size={16} /> Mock USB
                  </button>
                ) : null}
                {usbFirmwareOverridePending ? (
                  <button
                    className="secondary-button danger-action"
                    type="button"
                    onClick={() => void onUsbConnect(true)}
                    disabled={usbBusy}
                  >
                    Ignore warning and connect
                  </button>
                ) : null}
              </div>
              {usbMessage?.tone === "success" ? (
                <FeedbackMessage feedback={usbMessage} />
              ) : null}
            </div>
          </section>

          <section
            className={`connect-panel lan-fallback-panel ${showLanFallback ? "is-active" : ""}`}
          >
            <header className="connect-panel-header">
              <div>
                <h3>
                  <Globe2 size={18} /> LAN device API
                </h3>
                <p>
                  {showLanFallback
                    ? publicStaticBuild
                      ? "Primary browser-direct LAN path for the public static build"
                      : "Fallback for direct hardware HTTP/SSE when devd is unreachable"
                    : "Hidden during devd-backed discovery"}
                </p>
              </div>
              <span
                className={`transport-badge ${showLanFallback ? "http" : "offline"}`}
              >
                {showLanFallback ? "fallback" : "standby"}
              </span>
            </header>
            {showLanFallback ? (
              <>
                <form
                  className="connect-form compact"
                  onSubmit={onSubmit}
                  autoComplete="off"
                >
                  <label>
                    Target
                    <input
                      {...credentiallessInputProps}
                      name="lan-device-endpoint"
                      value={target}
                      onChange={(event) => setTarget(event.target.value)}
                      placeholder="mains-aegis-a1b2c3.local or 192.168.31.42"
                      inputMode="url"
                      autoCapitalize="none"
                      required
                    />
                  </label>
                  {publicStaticBuild ? (
                    <p className="field-help">
                      Chrome 142+ only. Enter hostname, FQDN, IPv4, or IPv4:port. The app will prefer verified hostnames and keep the IP as fallback.
                    </p>
                  ) : null}
                  <label>
                    Alias
                    <input
                      {...credentiallessInputProps}
                      name="lan-device-alias"
                      value={alias}
                      onChange={(event) => setAlias(event.target.value)}
                      placeholder="Lab rack A"
                    />
                  </label>
                  <label>
                    Location
                    <input
                      {...credentiallessInputProps}
                      name="lan-device-location"
                      value={location}
                      onChange={(event) => setLocation(event.target.value)}
                      placeholder="Bench 1"
                    />
                  </label>
                  <div className="form-actions with-callout">
                    <button
                      className="primary-button"
                      type="submit"
                      disabled={
                        busy ||
                        (publicStaticBuild && !browserLanCapability.supported)
                      }
                    >
                      <ButtonLabel
                        busy={busy}
                        busyText="Connecting"
                        text="Add LAN"
                      />
                    </button>
                    {message?.tone === "error" ? (
                      <ConnectionCallout
                        id="lan-connect-message"
                        message={message.message}
                      />
                    ) : null}
                    {demoMode ? (
                      <button
                        className="secondary-button"
                        type="button"
                        onClick={resetDemo}
                      >
                        Reset demo fleet
                      </button>
                    ) : null}
                  </div>
                </form>
                {message?.tone === "success" ? (
                  <FeedbackMessage feedback={message} />
                ) : null}
                {!browserLanCapability.supported && publicStaticBuild ? (
                  <ConnectionCallout
                    id="lan-capability-message"
                    message={`browser_lan_capability_required: ${browserLanCapability.reason ?? "Use Chrome 142+ in a secure context for LAN access."}`}
                  />
                ) : null}
              </>
            ) : (
              <div className="lan-standby-note">
                <Server size={16} />
                <span>
                  devd is handling LAN discovery. Manual entry stays disabled to
                  avoid duplicate targets.
                </span>
              </div>
            )}
          </section>
          {showLanScanPanel ? (
            <section className="connect-panel lan-scan-panel">
              <header className="connect-panel-header">
                <div>
                  <h3>
                    <Search size={18} /> CIDR scan
                  </h3>
                  <p>
                    Manually scan one IPv4 subnet from this browser. Results stay local to this session until you explicitly add a device.
                  </p>
                </div>
                <span className="transport-badge adapter">manual</span>
              </header>
              <form className="connect-form compact" onSubmit={onScanSubmit}>
                <label className="connect-field-full">
                  IPv4 CIDR
                  <input
                    {...credentiallessInputProps}
                    name="lan-cidr-scan"
                    value={cidr}
                    onChange={(event) => setCidr(event.target.value)}
                    placeholder="192.168.31.0/24"
                    autoCapitalize="none"
                    required
                  />
                </label>
                <ScanActionRow
                  busy={scanState.status === "scanning"}
                  disabled={
                    scanState.status === "scanning" ||
                    !browserLanCapability.supported
                  }
                  buttonText="Scan LAN"
                  busyText="Scanning"
                  successFeedback={
                    scanState.message?.tone === "success"
                      ? scanState.message
                      : null
                  }
                  errorMessage={
                    scanState.message?.tone === "error"
                      ? scanState.message.message
                      : null
                  }
                />
              </form>
              <div className="devd-device-list" aria-live="polite">
                {scanState.status === "done" &&
                scanState.candidates.length === 0 &&
                scanState.message?.tone !== "error" ? (
                  <div className="devd-empty-state">
                    <Radio size={18} />
                    <strong>No LAN candidates</strong>
                    <span>Try a different subnet or use a direct target.</span>
                  </div>
                ) : null}
                {scanState.candidates.map((candidate) => (
                  <article
                    className="devd-device-card scan-candidate-card"
                    key={candidate.key}
                  >
                    <div className="devd-device-main">
                      <span className="transport-badge http">LAN</span>
                      <div>
                        <h4>{candidate.alias}</h4>
                        <div className="devd-device-endpoints">
                          {candidate.endpoints.map((endpoint) => (
                            <p key={endpoint} title={endpoint}>
                              {endpoint}
                            </p>
                          ))}
                        </div>
                      </div>
                    </div>
                    <dl className="devd-device-meta scan-candidate-meta">
                      <div>
                        <dt>Device ID</dt>
                        <dd>{candidate.deviceId}</dd>
                      </div>
                      <div>
                        <dt>Status</dt>
                        <dd>{candidate.existingRecord ? "saved" : "new"}</dd>
                      </div>
                    </dl>
                    <div className="scan-candidate-footer">
                      <p className="scan-candidate-hint">
                        Select Add WiFi to save this LAN device.
                      </p>
                      <div className="devd-device-actions scan-candidate-actions">
                        {candidate.existingRecord ? (
                          <>
                            <button
                              className="primary-button small"
                              type="button"
                              onClick={() =>
                                navigate(deviceDefaultHref(candidate.existingRecord!))
                              }
                            >
                              Open
                            </button>
                            <button
                              className="secondary-button small"
                              type="button"
                              disabled={busy}
                              onClick={() => void onScanCandidateAdd(candidate)}
                            >
                              Add WiFi
                            </button>
                          </>
                        ) : (
                          <button
                            className="secondary-button small"
                            type="button"
                            disabled={busy}
                            onClick={() => void onScanCandidateAdd(candidate)}
                          >
                            Add WiFi
                          </button>
                        )}
                      </div>
                    </div>
                  </article>
                ))}
              </div>
            </section>
          ) : null}
        </div>
      ) : null}

      <div className="table-list">
        {records.map((record) => (
          <div className="table-row" key={record.target.deviceId}>
            <button
              className="table-row-main"
              type="button"
              onClick={() => navigate(deviceDefaultHref(record))}
              aria-label={`Open ${record.target.alias}`}
            >
              <strong>{record.target.alias}</strong>
              <span>{connectionEndpointLabel(record)}</span>
              {companionChannelSummary(record) ? (
                <span className="table-row-detail">
                  {companionChannelSummary(record)}
                </span>
              ) : null}
            </button>
            <div className="row-actions">
              <ConnectionBadges record={record} />
              {availableRecordChannels(record).length > 1
                ? (() => {
                    const channels = availableRecordChannels(record);
                    const recommendedTransport = channels[0];
                    const recommendedBusy =
                      savedDeviceSwitchTarget?.deviceId ===
                        record.target.deviceId &&
                      savedDeviceSwitchTarget.transport ===
                        recommendedTransport;
                    const recommendedActive =
                      activeRecordTransport(record) === recommendedTransport &&
                      record.connectionState === "online";
                    const switchBusy =
                      savedDeviceSwitchTarget?.deviceId ===
                      record.target.deviceId;
                    const otherOptions = channels.slice(1).map((transport) => {
                      const isBusy =
                        savedDeviceSwitchTarget?.deviceId ===
                          record.target.deviceId &&
                        savedDeviceSwitchTarget.transport === transport;
                      const isActive =
                        activeRecordTransport(record) === transport &&
                        record.connectionState === "online";
                      return {
                        value: transport,
                        label: channelUseText(transport, isActive),
                        disabled: switchBusy || isBusy || isActive,
                      };
                    });
                    return (
                      <div className="channel-switch-actions">
                        <button
                          className="secondary-button small"
                          type="button"
                          disabled={
                            recommendedActive || switchBusy
                          }
                          onClick={() =>
                            void onSavedDeviceChannelSwitch(
                              record,
                              recommendedTransport,
                            )
                          }
                        >
                          <ButtonLabel
                            busy={Boolean(recommendedBusy)}
                            busyText="Switching"
                            text={channelUseText(
                              recommendedTransport,
                              recommendedActive,
                            )}
                          />
                        </button>
                        {otherOptions.map((option) => (
                          <button
                            key={option.value}
                            className="secondary-button small"
                            type="button"
                            disabled={option.disabled}
                            onClick={() =>
                              void onSavedDeviceChannelSwitch(
                                record,
                                option.value as DeviceChannelTransport,
                              )
                            }
                          >
                            {option.label}
                          </button>
                        ))}
                      </div>
                    );
                  })()
                : null}
              <button
                className="icon-button"
                type="button"
                aria-label={`Refresh ${record.target.alias}`}
                title={`Refresh ${record.target.alias}`}
                onClick={() => void refreshDevice(record.target.deviceId)}
              >
                <RefreshCw size={16} />
              </button>
              {record.serial?.connected && record.serial.source !== "devd" ? (
                <button
                  className="icon-button"
                  type="button"
                  aria-label={`Disconnect ${record.target.alias}`}
                  title={`Disconnect ${record.target.alias}`}
                  onClick={() =>
                    void disconnectUsbSerialDevice(record.target.deviceId)
                  }
                >
                  <Cable size={16} />
                </button>
              ) : null}
              <button
                className="icon-button"
                type="button"
                aria-label={`Remove ${record.target.alias}`}
                title={`Remove ${record.target.alias}`}
                onClick={() => removeDevice(record.target.deviceId)}
              >
                <X size={16} />
              </button>
            </div>
          </div>
        ))}
      </div>
      {savedDeviceMessage ? (
        <FeedbackMessage feedback={savedDeviceMessage} />
      ) : null}
    </section>
  );
}

export function ConnectionCallout({
  id,
  message,
}: {
  id: string;
  message: string;
}) {
  const [code, body] = splitErrorMessage(message);
  const title =
    code === "serial_port_unavailable"
      ? "USB port is in use"
      : code === "firmware_artifact_mismatch"
        ? "Firmware mismatch"
        : code === "devd_http_service_requires_devd_panel"
          ? "Use the devd panel"
          : code === "browser_lan_capability_required"
            ? "Use Chrome or local devd UI"
          : "Connection failed";
  const guidance =
    code === "serial_port_unavailable"
      ? "Disconnect the devd session or close the app using this CDC port, then retry."
      : code === "firmware_artifact_mismatch"
        ? "Select matching firmware, flash the current build, or explicitly ignore this warning to continue."
        : code === "devd_http_service_requires_devd_panel"
          ? "LAN status connects directly to hardware over the device HTTP API. Use the devd panel only for mains-aegis-devd HTTP service endpoints."
          : code === "browser_lan_capability_required"
            ? "GitHub Pages browser-direct LAN access is only supported on Chrome 142+ in a secure context. Otherwise use the hosted or local devd UI."
          : "Check the selected device and try again.";

  return (
    <aside
      id={id}
      className="connection-callout"
      role="status"
      aria-live="polite"
    >
      <span className="connection-callout-anchor" aria-hidden="true" />
      <AlertTriangle size={15} />
      <span>
        <strong>{title}</strong>
        <span>{body || guidance}</span>
        {body ? <em>{guidance}</em> : null}
        {code ? <code>{code}</code> : null}
      </span>
    </aside>
  );
}

function DeviceOpenAction({
  navigateTo,
  menuLabel,
  options,
  onSelect,
}: {
  navigateTo: () => void;
  menuLabel: string;
  options: Array<{
    value: Extract<DeviceChannelTransport, "http" | "devd">;
    label: string;
    disabled: boolean;
  }>;
  onSelect: (transport: Extract<DeviceChannelTransport, "http" | "devd">) => void;
}) {
  const availableOptions = options.filter((option) => !option.disabled);
  return (
    <div className="split-action">
      <button className="primary-button small" type="button" onClick={navigateTo}>
        Open
      </button>
      {availableOptions.length > 0 ? (
        <details className="split-action-menu">
          <summary
            className="primary-button small split-action-trigger"
            aria-label={menuLabel}
            title={menuLabel}
          >
            <ChevronDown size={14} />
          </summary>
          <div className="split-action-popover" role="menu">
            {availableOptions.map((option) => (
              <button
                key={option.value}
                className="split-action-item"
                type="button"
                role="menuitem"
                onClick={() => onSelect(option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
        </details>
      ) : null}
    </div>
  );
}

function CompanionHelpBubble({
}: {
  mdnsHost?: string | null;
  ip?: string | null;
  port?: number | null;
}) {
  return (
    <span className="inline-companion-title-help">
      <button
        type="button"
        className="inline-companion-title-trigger"
        aria-label="Why is LAN binding suggested?"
      >
        Also bind LAN?
      </button>
      <span className="inline-companion-title-popover" aria-hidden="true">
        <strong>Why this is suggested</strong>
        <span>
          USB is already bound, and this device also answered on LAN with the
          same identity.
        </span>
        <span>You can add LAN now, or ignore this and keep using USB only.</span>
      </span>
    </span>
  );
}

export function WifiProvisioningCallout({
  id,
  progress,
  feedback,
}: {
  id: string;
  progress?: WifiProvisioningProgress | null;
  feedback?: UiFeedback | null;
}) {
  const isError = feedback?.tone === "error";
  const message =
    feedback?.message ??
    progress?.message ??
    "Waiting for hardware WiFi status";
  const [code, body] = splitErrorMessage(message);
  const title = wifiProvisioningTitle(isError, feedback, progress, message);

  return (
    <aside
      id={id}
      className={`wifi-progress-callout ${isError ? "is-error" : ""}`}
      role={isError ? "alert" : "status"}
      aria-live={isError ? "assertive" : "polite"}
    >
      <span className="connection-callout-anchor" aria-hidden="true" />
      {progress && !feedback ? (
        <Loader2 className="spin-icon" size={15} aria-hidden="true" />
      ) : isError ? (
        <AlertTriangle size={15} aria-hidden="true" />
      ) : (
        <Wifi size={15} aria-hidden="true" />
      )}
      <span>
        <strong>{title}</strong>
        <span>{body || message}</span>
        {progress?.network?.state ? (
          <em>
            Network state: {progress.network.state}
            {progress.network.ipv4 ? `, IP ${progress.network.ipv4}` : ""}
          </em>
        ) : null}
        {isError && code ? <code>{code}</code> : null}
      </span>
    </aside>
  );
}

function wifiProvisioningTitle(
  isError: boolean,
  feedback: UiFeedback | null | undefined,
  progress: WifiProvisioningProgress | null | undefined,
  message: string,
): string {
  if (isError) return "WiFi failed";
  if (feedback?.tone === "success")
    return message.toLowerCase().includes("cleared")
      ? "WiFi disabled"
      : "WiFi connected";
  if (progress?.phase === "connected") return "WiFi connected";
  if (progress?.phase === "disabled") return "WiFi disabled";
  if (progress?.phase === "ip") return "Getting IP address";
  if (progress?.phase === "clearing") return "Clearing WiFi";
  return "Connecting WiFi";
}

export function FeedbackMessage({ feedback }: { feedback: UiFeedback }) {
  return (
    <p
      className={`form-message ${feedback.tone === "error" ? "is-error" : "is-success"}`}
      role="status"
      aria-live="polite"
    >
      {feedback.message}
    </p>
  );
}

export function ScanActionRow({
  busy,
  disabled,
  buttonText,
  busyText,
  successFeedback,
  errorMessage,
}: {
  busy: boolean;
  disabled: boolean;
  buttonText: string;
  busyText: string;
  successFeedback: UiFeedback | null;
  errorMessage: string | null;
}) {
  return (
    <>
      <div className="form-actions with-callout scan-inline-actions">
        <button className="primary-button" type="submit" disabled={disabled}>
          <ButtonLabel busy={busy} busyText={busyText} text={buttonText} />
        </button>
        <span
          className="scan-inline-status"
          data-slot="scan-inline-status"
          role="status"
          aria-live="polite"
        >
          {successFeedback?.message ?? ""}
        </span>
        {errorMessage ? (
          <ConnectionCallout id="scan-connect-message" message={errorMessage} />
        ) : null}
      </div>
    </>
  );
}

export function ButtonLabel({
  icon: Icon,
  busy,
  busyText,
  text,
}: {
  icon?: LucideIcon;
  busy: boolean;
  busyText: string;
  text: string;
}) {
  const LabelIcon = busy ? Loader2 : Icon;
  return (
    <>
      {LabelIcon ? (
        <LabelIcon
          className={busy ? "spin-icon" : undefined}
          size={16}
          aria-hidden="true"
        />
      ) : null}
      {busy ? busyText : text}
    </>
  );
}

function successFeedback(message: string): UiFeedback {
  return { tone: "success", message };
}

function errorFeedback(error: DeviceRecord["error"]): UiFeedback {
  return {
    tone: "error",
    message: `${error?.code ?? "error"}: ${error?.message ?? "Operation failed"}`,
  };
}

function manualChargeControlFeedback(
  error: DeviceRecord["error"],
): UiFeedback {
  const detail = chargeControlDetailFromPayload(error?.details);
  if (detail?.readiness.block?.message) {
    return {
      tone: "error",
      message: `${sentenceWithTerminator(detail.readiness.block.message)} Planned path: ${chargeControlPathLabel(detail)}.`,
    };
  }
  if (detail?.readiness.state === "confirm_required") {
    return {
      tone: "error",
      message: chargeControlSummaryText(detail),
    };
  }
  return errorFeedback(error);
}

function DeviceOverviewPage({ record }: { record: DeviceRecord }) {
  const status = record.status;
  return (
    <section className="page-flow" data-evidence-target="device-overview">
      <div className="detail-grid">
        <InfoPanel title="Input" icon={PlugZap}>
          <MetricLine
            label="Mains"
            value={boolLabel(status?.input.mains_present, "present", "absent")}
          />
          <MetricLine
            label="VBUS"
            value={formatVoltage(status?.input.input_vbus_mv)}
          />
          <MetricLine
            label="IBUS"
            value={formatCurrent(status?.input.input_ibus_ma)}
          />
        </InfoPanel>
        <InfoPanel title="Battery" icon={BatteryCharging}>
          <MetricLine
            label="SOC"
            value={formatPercent(status?.battery.soc_pct)}
          />
          <MetricLine
            label="Pack"
            value={formatVoltage(status?.battery.pack_mv)}
          />
          <MetricLine
            label="Ready"
            value={boolLabel(status?.battery.discharge_ready, "yes", "no")}
          />
        </InfoPanel>
        <InfoPanel title="Outputs" icon={Cable}>
          <MetricLine label="Active" value={status?.output.active ?? "--"} />
          <MetricLine
            label="OUT A"
            value={channelLabel(status?.output.out_a)}
          />
          <MetricLine
            label="OUT B"
            value={channelLabel(status?.output.out_b)}
          />
        </InfoPanel>
        <InfoPanel title="Thermal" icon={Thermometer}>
          <MetricLine
            label="TMP A"
            value={`${formatTemp(status?.thermal.tmp_a_c)} / ${status?.thermal.tmp_a_state ?? "--"}`}
          />
          <MetricLine
            label="TMP B"
            value={`${formatTemp(status?.thermal.tmp_b_c)} / ${status?.thermal.tmp_b_state ?? "--"}`}
          />
          <MetricLine label="Max" value={formatTemp(maxTemp(status))} />
        </InfoPanel>
      </div>
    </section>
  );
}

export const ACTIVE_ALERT_REFRESH_MS = 2_000;
export const ACTIVE_ALERT_REQUEST_TIMEOUT_MS = 1_500;

export function activeAlertSeverity(
  snapshot: ActiveAlertsSnapshot | null,
): "warning" | "critical" | null {
  if (!snapshot || snapshot.alerts.length === 0) return null;
  return snapshot.alerts.some((alert) => alert.severity === "critical")
    ? "critical"
    : "warning";
}

export function audibleAlertCount(snapshot: ActiveAlertsSnapshot | null): number {
  return snapshot?.alerts.filter((alert) => alert.sound_state === "audible").length ?? 0;
}

type FleetActiveAlertEntry = {
  record: DeviceRecord;
  snapshot: ActiveAlertsSnapshot;
  lastUpdated: string | null;
  refreshError: string | null;
};

type FleetActiveAlertsState = {
  snapshots: Record<string, ActiveAlertsSnapshot>;
  active: FleetActiveAlertEntry[];
};

function useFleetActiveAlerts(records: DeviceRecord[]): FleetActiveAlertsState {
  const { getSerialAlerts } = useDeviceRegistry();
  const recordsRef = useRef(records);
  recordsRef.current = records;
  const [snapshots, setSnapshots] = useState<
    Record<string, ActiveAlertsSnapshot>
  >({});
  const [lastUpdated, setLastUpdated] = useState<Record<string, string>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const refreshGeneration = useRef(0);
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const recordKey = records
    .map((record) =>
      [
        record.target.deviceId,
        record.connectionState,
        record.target.transport,
        record.target.baseUrl,
        record.target.rememberedChannels?.devd?.baseUrl ?? "",
        record.target.rememberedChannels?.http?.baseUrl ?? "",
        record.target.rememberedChannels?.http?.fallbackBaseUrl ?? "",
        record.serial?.source ?? "",
        record.serial?.leaseId ?? "",
        record.runtimeId ?? "",
      ].join(":"),
    )
    .sort()
    .join("|");

  const refresh = useCallback(() => {
    if (refreshInFlight.current) return refreshInFlight.current;
    const request = (async () => {
      const generation = ++refreshGeneration.current;
      const currentRecords = recordsRef.current;
      const results = await Promise.all(
        currentRecords.map(async (record) => {
          const targets = alertControlTargets(record);
          if (record.connectionState === "offline" || targets.length === 0)
            return { record, snapshot: null, error: null };
          try {
            const snapshot = await readAlertsFromTargets(targets, getSerialAlerts);
            return { record, snapshot, error: null };
          } catch (cause) {
            return {
              record,
              snapshot: null,
              error: alertErrorMessage(cause),
              clearSnapshot: alertErrorClearsSnapshot(cause),
            };
          }
        }),
      );
      if (generation !== refreshGeneration.current) return;
      const liveIds = new Set(currentRecords.map((record) => record.target.deviceId));
      setSnapshots((current) => {
        const next = Object.fromEntries(
          Object.entries(current).filter(([deviceId]) => liveIds.has(deviceId)),
        );
        for (const result of results) {
          const deviceId = result.record.target.deviceId;
          if (result.snapshot) next[deviceId] = result.snapshot;
          else if (!result.error || result.clearSnapshot) delete next[deviceId];
        }
        return next;
      });
      setLastUpdated((current) => {
        const next = { ...current };
        const now = new Date().toISOString();
        for (const result of results) {
          const deviceId = result.record.target.deviceId;
          if (result.snapshot) next[deviceId] = now;
          if (!liveIds.has(deviceId)) delete next[deviceId];
        }
        return next;
      });
      setErrors((current) => {
        const next = { ...current };
        for (const result of results) {
          const deviceId = result.record.target.deviceId;
          if (result.error) next[deviceId] = result.error;
          else delete next[deviceId];
        }
        for (const deviceId of Object.keys(next)) {
          if (!liveIds.has(deviceId)) delete next[deviceId];
        }
        return next;
      });
    })();
    const trackedRequest = request.finally(() => {
      if (refreshInFlight.current === trackedRequest) refreshInFlight.current = null;
    });
    refreshInFlight.current = trackedRequest;
    return trackedRequest;
  }, [getSerialAlerts]);

  useEffect(() => {
    refreshGeneration.current += 1;
    refreshInFlight.current = null;
    void refresh();
    return () => {
      // Drop results from the previous fleet/transport set before it can repopulate stale badges.
      refreshGeneration.current += 1;
      refreshInFlight.current = null;
    };
  }, [recordKey, refresh]);

  useEffect(() => {
    const interval = window.setInterval(
      () => void refresh(),
      ACTIVE_ALERT_REFRESH_MS,
    );
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [refresh]);

  const active = records.flatMap((record) => {
    const snapshot = snapshots[record.target.deviceId];
    if (!snapshot || snapshot.alerts.length === 0) return [];
    return [
      {
        record,
        snapshot,
        lastUpdated: lastUpdated[record.target.deviceId] ?? null,
        refreshError: errors[record.target.deviceId] ?? null,
      },
    ];
  });
  return { snapshots, active };
}

type ActiveAlertsViewState = {
  snapshot: ActiveAlertsSnapshot | null;
  loading: boolean;
  error: string | null;
  lastUpdated: string | null;
  refresh: (options?: {
    background?: boolean;
    preserveError?: boolean;
  }) => Promise<void>;
};

function useActiveAlertsSnapshot(
  record: DeviceRecord | null,
): ActiveAlertsViewState {
  const { getSerialAlerts } = useDeviceRegistry();
  const deviceId = record?.target.deviceId ?? null;
  const connectionState = record?.connectionState ?? "offline";
  const [snapshot, setSnapshot] = useState<ActiveAlertsSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<string | null>(null);
  const refreshGeneration = useRef(0);
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const targets = useMemo(
    () => (record ? alertControlTargets(record) : []),
    [
      deviceId,
      record?.runtimeId,
      record?.serial?.leaseId,
      record?.serial?.baseUrl,
      record?.serial?.source,
      record?.target.baseUrl,
      record?.target.rememberedChannels?.devd?.baseUrl,
      record?.target.rememberedChannels?.devd?.devdDeviceId,
      record?.target.rememberedChannels?.http?.baseUrl,
      record?.target.rememberedChannels?.http?.fallbackBaseUrl,
      record?.target.transport,
    ],
  );

  const refresh = useCallback(
    (options?: { background?: boolean; preserveError?: boolean }) => {
      if (refreshInFlight.current) return refreshInFlight.current;
      const request = (async () => {
        const generation = ++refreshGeneration.current;
        if (!deviceId || connectionState === "offline") {
          setSnapshot(null);
          setLastUpdated(null);
          setLoading(false);
          setError(
            deviceId
              ? "Device offline. Reconnect to view or mute active alerts."
              : null,
          );
          return;
        }
        if (targets.length === 0) {
          setSnapshot(null);
          setLastUpdated(null);
          setLoading(false);
          setError("This connected device does not expose the alerts contract yet.");
          return;
        }
        if (!options?.background) setLoading(true);
        try {
          const nextSnapshot = await readAlertsFromTargets(targets, getSerialAlerts);
          if (generation !== refreshGeneration.current) return;
          setSnapshot(nextSnapshot);
          setLastUpdated(new Date().toISOString());
          if (!options?.preserveError) setError(null);
        } catch (cause) {
          if (generation !== refreshGeneration.current) return;
          if (alertErrorClearsSnapshot(cause)) {
            setSnapshot(null);
            setLastUpdated(null);
          }
          setError(alertErrorMessage(cause));
        } finally {
          if (generation === refreshGeneration.current) setLoading(false);
        }
      })();
      const trackedRequest = request.finally(() => {
        if (refreshInFlight.current === trackedRequest) refreshInFlight.current = null;
      });
      refreshInFlight.current = trackedRequest;
      return trackedRequest;
    },
    [connectionState, deviceId, getSerialAlerts, targets],
  );

  useEffect(() => {
    setSnapshot(null);
    setLastUpdated(null);
    setError(null);
    void refresh();
    return () => {
      // Invalidate an in-flight read before the selected device or transport changes.
      refreshGeneration.current += 1;
      refreshInFlight.current = null;
    };
  }, [deviceId, refresh]);

  useEffect(() => {
    if (!deviceId || connectionState === "offline" || targets.length === 0)
      return undefined;
    const interval = window.setInterval(
      () => void refresh({ background: true }),
      ACTIVE_ALERT_REFRESH_MS,
    );
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible")
        void refresh({ background: true });
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [connectionState, deviceId, refresh, targets]);

  return { snapshot, loading, error, lastUpdated, refresh };
}

const alertCopy: Record<ActiveAlert["alert_id"], { title: string; summary: string }> = {
  mains_absent_dc: { title: "Mains absent", summary: "DC input power is unavailable." },
  high_stress: { title: "High stress", summary: "The system is operating under thermal stress." },
  battery_low_no_mains: { title: "Battery low", summary: "Battery is low with no mains input." },
  battery_low_with_mains: { title: "Battery low", summary: "Battery remains low while mains is present." },
  shutdown_protection: { title: "Shutdown protection", summary: "A protection shutdown is active." },
  io_over_voltage: { title: "Output over-voltage", summary: "An output voltage limit was exceeded." },
  io_over_current: { title: "Output over-current", summary: "An output current limit was exceeded." },
  module_fault: { title: "Module fault", summary: "A required hardware module reported a fault." },
  battery_protection: { title: "Battery protection", summary: "The battery protection path is active." },
};

function AlertsPage({
  record,
  state,
}: {
  record: DeviceRecord;
  state: ActiveAlertsViewState;
}) {
  const { muteSerialAlert } = useDeviceRegistry();
  const { snapshot, loading, error, lastUpdated, refresh } = state;
  const [muting, setMuting] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const targets = useMemo(() => alertControlTargets(record), [record]);

  const mute = async (alert: ActiveAlert) => {
    if (targets.length === 0 || error || alert.sound_state === "muted") return;
    setMuting(alert.alert_id);
    setActionError(null);
    try {
      await muteAlertFromTargets(
        targets,
        alert,
        muteSerialAlert,
      );
      await refresh();
    } catch (cause) {
      setActionError(alertErrorMessage(cause));
      await refresh({ preserveError: true });
    } finally {
      setMuting(null);
    }
  };

  const alerts = snapshot?.alerts ?? [];
  return (
    <section className="page-flow alerts-page" data-evidence-target="device-alerts">
      <section className="info-panel alerts-panel">
        <div className="alerts-heading">
          <div>
            <span className="eyebrow">Active alerts</span>
            <h2>{alerts.length === 0 ? "No active alerts" : `${alerts.length} active`}</h2>
            <span className="alerts-live-status">
              Auto-updates every 2 seconds
              {lastUpdated ? ` · updated ${timeAgo(lastUpdated)}` : ""}
            </span>
          </div>
          <button className="icon-button" type="button" onClick={() => void refresh()} title="Refresh alerts" aria-label="Refresh alerts">
            <RefreshCw size={18} className={loading ? "spin-icon" : ""} />
          </button>
        </div>
        {error || actionError ? <p className="form-message is-error" role="alert">{actionError ?? error}</p> : null}
        {!loading && alerts.length === 0 && !error ? (
          <div className="alerts-empty"><BellRing size={24} /><span>All monitored conditions are clear.</span></div>
        ) : null}
        <div className="alerts-list">
          {alerts.map((alert) => {
            const copy = alertCopy[alert.alert_id];
            const busy = muting === alert.alert_id;
            return (
              <article className={`alert-row is-${alert.severity}`} key={`${alert.alert_id}:${alert.instance_id}`}>
                <AlertTriangle size={20} aria-hidden="true" />
                <div className="alert-row-copy">
                  <div><strong>{copy.title}</strong><span className="alert-severity">{alert.severity}</span></div>
                  <p>{alert.summary ?? copy.summary}</p>
                </div>
                <div className="alert-sound">
                  <span>{alertSoundLabel(alert.sound_state)}</span>
                  <button
                    type="button"
                    className="icon-button"
                    disabled={Boolean(error) || alert.sound_state === "muted" || busy}
                    onClick={() => void mute(alert)}
                    title={error ? "Refresh alerts before muting" : alert.sound_state === "audible" ? "Mute this alert" : alertSoundLabel(alert.sound_state)}
                    aria-label={`Mute ${copy.title}`}
                  >
                    {busy ? <Loader2 className="spin-icon" size={18} /> : alert.sound_state === "audible" ? <Volume2 size={18} /> : <VolumeX size={18} />}
                  </button>
                </div>
              </article>
            );
          })}
        </div>
      </section>
    </section>
  );
}

type AlertControlTarget =
  | { kind: "devd"; baseUrl: string; deviceId: string }
  | { kind: "http"; baseUrls: string[] }
  | { kind: "serial"; deviceId: string };

function alertControlTargets(record: DeviceRecord): AlertControlTarget[] {
  const targets: AlertControlTarget[] = [];
  const seen = new Set<string>();
  const add = (target: AlertControlTarget) => {
    const key = target.kind === "http"
      ? `${target.kind}:${target.baseUrls.join(",")}`
      : `${target.kind}:${target.deviceId}:${"baseUrl" in target ? target.baseUrl : ""}`;
    if (!seen.has(key)) {
      seen.add(key);
      targets.push(target);
    }
  };
  const httpBaseUrls = rememberedHttpBaseUrls(record);
  const active = activeRecordTransport(record);
  const addTransport = (transport: DeviceChannelTransport) => {
    if (transport === "serial") {
      if (record.serial?.source === "web_serial") {
        add({ kind: "serial", deviceId: record.target.deviceId });
      } else if (record.target.mock && record.serial?.source === "mock") {
        add({ kind: "http", baseUrls: [`mock:usb-alerts:${record.target.deviceId}`] });
      }
      return;
    }
    if (transport === "devd") {
      const baseUrl = record.serial?.connected && record.serial.source === "devd"
        ? record.serial.baseUrl
        : record.target.transport === "devd"
          ? record.target.baseUrl
          : record.target.rememberedChannels?.devd?.baseUrl;
      if (baseUrl) {
        add({
          kind: "devd",
          baseUrl,
          deviceId:
            record.target.rememberedChannels?.devd?.devdDeviceId ??
            record.target.deviceId,
        });
      }
      return;
    }
    if (httpBaseUrls.length > 0) add({ kind: "http", baseUrls: httpBaseUrls });
  };
  if (active) addTransport(active);
  for (const transport of ["devd", "http", "serial"] as DeviceChannelTransport[]) {
    if (transport !== active) addTransport(transport);
  }
  return targets;
}

async function withAlertTargetFallback<T>(
  targets: AlertControlTarget[],
  operation: (target: AlertControlTarget) => Promise<T>,
): Promise<T> {
  let lastError: unknown = null;
  for (const target of targets) {
    try {
      return await operation(target);
    } catch (cause) {
      lastError = cause;
      if (!toErrorEnvelope(cause).retryable) throw cause;
    }
  }
  throw lastError ?? new Error("No alerts transport is available");
}

async function withAlertHttpFallback<T>(
  baseUrls: string[],
  operation: (baseUrl: string) => Promise<T>,
): Promise<T> {
  let lastError: unknown = null;
  for (const baseUrl of baseUrls) {
    try {
      return await operation(baseUrl);
    } catch (cause) {
      lastError = cause;
      if (!toErrorEnvelope(cause).retryable) throw cause;
    }
  }
  throw lastError ?? new Error("No HTTP alerts transport is available");
}

async function readAlertsFromTargets(
  targets: AlertControlTarget[],
  getSerialAlerts: (deviceId: string) => Promise<ActiveAlertsSnapshot>,
): Promise<ActiveAlertsSnapshot> {
  return withAlertTargetFallback(targets, (target) =>
    target.kind === "devd"
      ? getDevdDeviceAlerts(target.baseUrl, target.deviceId, {
          timeoutMs: ACTIVE_ALERT_REQUEST_TIMEOUT_MS,
        })
      : target.kind === "serial"
        ? getSerialAlerts(target.deviceId)
        : withAlertHttpFallback(
            target.baseUrls,
            (baseUrl) =>
              getDeviceAlerts(baseUrl, {
                timeoutMs: ACTIVE_ALERT_REQUEST_TIMEOUT_MS,
              }),
          ),
  );
}

async function muteAlertFromTargets(
  targets: AlertControlTarget[],
  alert: ActiveAlert,
  muteSerialAlert: (
    deviceId: string,
    alertId: string,
    instanceId: number,
  ) => Promise<unknown>,
): Promise<void> {
  await withAlertTargetFallback(targets, (target) =>
    target.kind === "devd"
      ? muteDevdDeviceAlert(
          target.baseUrl,
          target.deviceId,
          alert.alert_id,
          alert.instance_id,
        )
      : target.kind === "serial"
        ? muteSerialAlert(target.deviceId, alert.alert_id, alert.instance_id)
        : withAlertHttpFallback(
            target.baseUrls,
            (baseUrl) => muteDeviceAlert(baseUrl, alert.alert_id, alert.instance_id),
          ),
  );
}

function alertErrorClearsSnapshot(cause: unknown): boolean {
  const code = toErrorEnvelope(cause).code;
  return code === "unsupported" || code === "unsupported_operation";
}

function alertSoundLabel(sound: ActiveAlert["sound_state"]): string {
  if (sound === "audible") return "Sounding";
  if (sound === "muted") return "Muted";
  if (sound === "system_silent") return "System silent";
  return "Policy silent";
}

function alertErrorMessage(cause: unknown): string {
  const envelope = toErrorEnvelope(cause);
  if (
    envelope.code === "unsupported_operation" ||
    envelope.code === "unsupported"
  ) {
    return "Alerts are unavailable on this firmware. Upgrade the device to enable per-alert muting.";
  }
  if (envelope.code.includes("stale")) {
    return "The alert changed before it could be muted. The list has been refreshed.";
  }
  if (envelope.code.includes("inactive")) {
    return "The alert cleared before it could be muted. The list has been refreshed.";
  }
  return `${envelope.code}: ${envelope.message}`;
}

function PowerPage({ record }: { record: DeviceRecord }) {
  const {
    setManualChargePrefs,
    refreshChargeControlDetail,
    previewManualCharge,
    controlManualCharge,
  } = useDeviceRegistry();
  const status = record.status;
  const settings = record.settings;
  const liveDetail = record.chargeControlDetail;
  const [manualPrefs, setManualPrefs] = useState<DeviceSettings["manual_charge"]>(
    settings?.manual_charge ?? defaultManualChargePrefs(),
  );
  const [feedback, setFeedback] = useState<UiFeedback | null>(null);
  const [dialogFeedback, setDialogFeedback] = useState<UiFeedback | null>(null);
  const [busy, setBusy] = useState<
    "request-save" | "request-start" | "stop" | "confirm" | null
  >(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [requestDialogOpen, setRequestDialogOpen] = useState(false);
  const [previewDetail, setPreviewDetail] = useState<ChargeControlDetail | null>(
    null,
  );
  const refreshChargeControlDetailRef = useRef(refreshChargeControlDetail);
  const chargeControlRefreshKey = [
    record.target.deviceId,
    record.target.transport ?? "",
    record.target.preferredTransport ?? "",
    record.target.baseUrl,
    record.target.rememberedChannels?.devd?.baseUrl ?? "",
    record.target.rememberedChannels?.devd?.devdDeviceId ?? "",
    record.serial?.source ?? "",
    record.serial?.baseUrl ?? "",
    record.serial?.leaseId ?? "",
    record.runtimeId ?? "",
    record.serial?.connected ? "connected" : "disconnected",
  ].join("|");

  useEffect(() => {
    refreshChargeControlDetailRef.current = refreshChargeControlDetail;
  }, [refreshChargeControlDetail]);

  useEffect(() => {
    if (!settings?.manual_charge) return;
    setManualPrefs({
      ...settings.manual_charge,
      power_path: settings.manual_charge.power_path ?? "auto",
    });
  }, [settings]);

  useEffect(() => {
    let cancelled = false;
    let retryTimer: number | null = null;
    let retriedBootstrap = false;
    const runRefresh = () => {
      void refreshChargeControlDetailRef.current(record.target.deviceId).then(
        (result) => {
          if (cancelled) return;
          if (result.ok || result.detail) {
            setFeedback(null);
            return;
          }
          const bootstrapRetry =
            !retriedBootstrap &&
            (record.target.temporary ||
              record.target.transport === "devd" ||
              record.target.preferredTransport === "devd") &&
            (result.error?.code === "serial_session_required" ||
              result.error?.code === "devd_channel_unavailable");
          if (bootstrapRetry) {
            retriedBootstrap = true;
            retryTimer = window.setTimeout(() => {
              retryTimer = null;
              runRefresh();
            }, 500);
            return;
          }
          setFeedback(errorFeedback(result.error));
        },
      );
    };
    runRefresh();
    return () => {
      cancelled = true;
      if (retryTimer !== null) window.clearTimeout(retryTimer);
    };
  }, [chargeControlRefreshKey, record.target.deviceId]);

  useEffect(() => {
    if (!requestDialogOpen) {
      setPreviewBusy(false);
      setPreviewDetail(null);
      setDialogFeedback(null);
      return;
    }
    if (liveDetail?.summary.manual_active) {
      setPreviewBusy(false);
      setPreviewDetail(liveDetail);
      setDialogFeedback(null);
      return;
    }
    let cancelled = false;
    setPreviewBusy(true);
    setDialogFeedback(null);
    void previewManualCharge(record.target.deviceId, manualPrefs).then((result) => {
      if (cancelled) return;
      setPreviewBusy(false);
      setPreviewDetail(result.detail ?? null);
      if (!result.ok && !result.detail) {
        setDialogFeedback(errorFeedback(result.error));
      }
    });
    return () => {
      cancelled = true;
    };
  }, [
    requestDialogOpen,
    record.target.deviceId,
    record.runtimeId,
    record.serial?.leaseId,
    manualPrefs.target,
    manualPrefs.speed,
    manualPrefs.timer_h,
    manualPrefs.power_path,
    liveDetail?.summary.manual_active,
  ]);

  const pressureScore = Math.max(0, Math.min(100, status?.input.pressure_score_pct ?? 0));
  const pressureState = status?.input.pressure_state ?? "--";
  const limitReason = status?.charger.limit_reason ?? "none";
  const pressureSeverity = pressureSeverityForState(status?.input.pressure_state);
  const tpsTotalIoutMa = status?.input.tps_total_iout_ma ?? null;
  const tpsLimitThresholdMa =
    status?.input.tps_limit_threshold_ma ?? status?.charger.limit_threshold_ma ?? null;
  const pressureReasonLabel = powerReasonLabel(status?.input.pressure_reason);
  const limitReasonLabel = powerReasonLabel(limitReason);
  const stopSummary = powerStopSummary(
    status?.input.pressure_reason,
    tpsTotalIoutMa,
    tpsLimitThresholdMa,
  );
  const chargeControlTone = chargeControlSeverity(liveDetail);
  const chargeControlLoading = liveDetail == null;
  const requestDetail = previewDetail ?? liveDetail;
  const currentReason =
    liveDetail?.readiness.block?.message ??
    powerReasonLabel(liveDetail?.summary.last_stop_reason);
  const directEvidence = chargeControlEvidenceText(liveDetail);
  const directEvidenceEntries = chargeControlEvidenceEntries(liveDetail);
  const chargeCurrentSource = chargePowerPathLabel(
    liveDetail?.telemetry.input_source ?? status?.input.source,
  );
  const chargeBoundPath = chargeControlPathLabel(liveDetail);
  const chargePolicyTarget = formatCurrent(
    liveDetail?.telemetry.policy_target_ichg_ma,
  );
  const chargeInputLimitSummary = liveDetail?.telemetry.input_limit_summary ?? "--";
  const chargeIbatActual = formatCurrent(liveDetail?.telemetry.ibat_actual_ma);
  const chargeLoopLabel = chargeControlLoopLabel(liveDetail);
  const controlDetailEntries = [
    {
      label: "Target voltage",
      value: formatVoltage(liveDetail?.telemetry.target_voltage_mv),
    },
    {
      label: "Input limit",
      value: liveDetail?.telemetry.input_limit_summary ?? "--",
    },
    {
      label: "Output limit",
      value: liveDetail?.telemetry.output_limit_summary ?? "--",
    },
    {
      label: "VINDPM",
      value: formatVoltage(liveDetail?.telemetry.vindpm_mv),
    },
    {
      label: "IINDPM",
      value: formatCurrent(liveDetail?.telemetry.iindpm_ma),
    },
    {
      label: "Output power",
      value: formatPowerWatts(liveDetail?.telemetry.output_power_w10),
    },
  ];
  const readinessEntries = [
    { label: "Current reason", value: currentReason },
    {
      label: "Loop avoid",
      value: chargeControlLoopLabel(liveDetail),
    },
    {
      label: "Remaining",
      value: formatRemainingMinutes(liveDetail?.summary.remaining_minutes),
    },
  ];

  async function persistManualChargePrefs(
    busyState: "request-save" | "request-start" | "confirm",
  ): Promise<boolean> {
    setBusy(busyState);
    setDialogFeedback(null);
    const result = await setManualChargePrefs(record.target.deviceId, manualPrefs);
    if (!result.ok) {
      setBusy(null);
      setDialogFeedback(errorFeedback(result.error));
      return false;
    }
    return true;
  }

  async function onRequestDialogSaveDefaults() {
    const saved = await persistManualChargePrefs("request-save");
    const refreshed = saved
      ? await refreshChargeControlDetail(record.target.deviceId)
      : null;
    setBusy(null);
    if (!saved) return;
    if (refreshed && !refreshed.ok && !refreshed.detail) {
      setFeedback(errorFeedback(refreshed.error));
      return;
    }
    setRequestDialogOpen(false);
    setFeedback(successFeedback("Manual charge defaults saved"));
  }

  async function onRequestDialogStart(confirmLoop = false) {
    const saved = await persistManualChargePrefs(
      confirmLoop ? "confirm" : "request-start",
    );
    if (!saved) return;
    const result = await controlManualCharge(record.target.deviceId, {
      action: "start",
      confirm_loop: confirmLoop || undefined,
    });
    setBusy(null);
    if (result.ok) {
      setRequestDialogOpen(false);
      setPreviewDetail(null);
      setDialogFeedback(null);
      setFeedback(successFeedback(result.message ?? "Manual charge started"));
      return;
    }
    if (result.detail) {
      setPreviewDetail(result.detail);
      if (result.detail.readiness.state === "confirm_required") {
        return;
      }
    }
    if (result.error) {
      setDialogFeedback(manualChargeControlFeedback(result.error));
    }
  }

  async function onRequestDialogStop() {
    setBusy("stop");
    setDialogFeedback(null);
    const result = await controlManualCharge(record.target.deviceId, {
      action: "stop",
    });
    setBusy(null);
    if (result.ok) {
      setRequestDialogOpen(false);
      setPreviewDetail(null);
      setFeedback(successFeedback(result.message ?? "Manual charge stopped"));
      return;
    }
    if (result.detail) setPreviewDetail(result.detail);
    if (result.error) {
      setDialogFeedback(manualChargeControlFeedback(result.error));
    }
  }

  return (
    <section className="page-flow">
      <section className="power-domain-section" data-evidence-target="power-charging">
        <div className="power-domain-header">
          <span className="power-domain-icon">
            <BatteryCharging size={18} />
          </span>
          <div>
            <h2>Charging</h2>
            <p>
              Manual and automatic charge control, selected input path, source
              pressure, and charger runtime.
            </p>
          </div>
        </div>

        <section
          className="info-panel charge-control-panel"
          data-evidence-target="charge-control"
        >
          <header>
            <BatteryCharging size={18} />
            <h2>Charge Control</h2>
          </header>
          <div className="charge-control-shell">
            <div className="charge-control-overview">
              <div className="charge-control-hero">
                <div className="charge-control-status">
                  <span className={`severity-badge severity-${chargeControlTone}`}>
                    {chargeModeLabel(liveDetail)}
                  </span>
                  <strong>{chargeControlHeadline(liveDetail)}</strong>
                  <p>{chargeControlSummaryText(liveDetail)}</p>
                </div>
              </div>
              <div className="charge-control-kpis charge-control-kpis-overview">
                <div className="charge-kpi">
                  <span>Current source</span>
                  <strong>{chargeCurrentSource}</strong>
                  <em>{status?.charger.state ?? "--"}</em>
                </div>
                <div className="charge-kpi">
                  <span>Bound path</span>
                  <strong>{chargeBoundPath}</strong>
                  <em>{chargePowerPathLabel(status?.input.source)}</em>
                </div>
                <div className="charge-kpi">
                  <span>Policy target</span>
                  <strong>{chargePolicyTarget}</strong>
                  <em>{chargeInputLimitSummary}</em>
                </div>
                <div className="charge-kpi">
                  <span>IBAT actual</span>
                  <strong>{chargeIbatActual}</strong>
                  <em>{chargeLoopLabel}</em>
                </div>
              </div>
            </div>

            <div className="charge-control-body">
              <div className="charge-control-main">
                <section className="charge-control-section">
                  <h3>Control detail</h3>
                  <div className="charge-control-fact-grid">
                    {controlDetailEntries.map((entry) => (
                      <div className="charge-control-fact" key={entry.label}>
                        <span>{entry.label}</span>
                        <strong>{entry.value}</strong>
                      </div>
                    ))}
                  </div>
                </section>
                {directEvidenceEntries.length ? (
                  <section className="charge-control-section">
                    <h3>Direct evidence</h3>
                    <div className="charge-evidence-grid">
                      {directEvidenceEntries.map((entry) => (
                        <div
                          className="charge-evidence-card"
                          key={`${entry.source}:${entry.code}`}
                        >
                          <span>{entry.label}</span>
                          <strong>{chargeControlEvidenceValue(entry.value)}</strong>
                        </div>
                      ))}
                    </div>
                  </section>
                ) : directEvidence ? (
                  <section className="charge-control-section">
                    <h3>Direct evidence</h3>
                    <p className="charge-control-section-copy">{directEvidence}</p>
                  </section>
                ) : null}
              </div>
              <aside className="charge-control-rail" aria-label="Charge readiness and evidence">
                <div className="charge-control-rail-group charge-control-rail-action">
                  <h3>Next step</h3>
                  <p className="charge-control-rail-copy">
                    {chargeControlActionHint(liveDetail)}
                  </p>
                  <div className="form-actions charge-control-actions charge-control-actions-inline">
                    <button
                      className="primary-button"
                      data-importance="critical"
                      type="button"
                      disabled={busy !== null}
                      onClick={() => {
                        setFeedback(null);
                        setDialogFeedback(null);
                        setPreviewDetail(null);
                        setRequestDialogOpen(true);
                      }}
                    >
                      {liveDetail?.summary.manual_active
                        ? "Manage manual charge"
                        : "Request manual charge"}
                    </button>
                  </div>
                  {feedback ? <FeedbackMessage feedback={feedback} /> : null}
                </div>

                <div className="charge-control-rail-group">
                  <h3>Readiness</h3>
                  <p className="charge-control-rail-copy">
                    {chargeControlReadinessHint(liveDetail)}
                  </p>
                  <div className="charge-control-readiness-grid">
                    {readinessEntries.map((entry) => (
                      <div className="charge-control-readiness-card" key={entry.label}>
                        <span>{entry.label}</span>
                        <strong>{entry.value}</strong>
                      </div>
                    ))}
                  </div>
                </div>
              </aside>
            </div>
          </div>
        </section>

        <div className="detail-grid charge-diagnostics-grid">
          <InfoPanel title="Input" icon={PlugZap}>
            <div className="power-pressure-summary">
              <span className={`severity-badge severity-${pressureSeverity}`}>
                {pressureState}
              </span>
              <strong>{pressureScore}%</strong>
              <span>{pressureReasonLabel}</span>
            </div>
            <div
              className="power-pressure-bar"
              aria-label={`Input pressure ${pressureScore}%`}
            >
              <span
                className={`power-pressure-fill power-pressure-fill-${pressureSeverity}`}
                style={{ width: `${pressureScore}%` }}
              />
            </div>
            <MetricLine label="Source" value={status?.input.source ?? "--"} />
            <MetricLine
              label="Mains present"
              value={boolLabel(status?.input.mains_present, "yes", "no")}
            />
            <MetricLine
              label="Pre-TPS VIN"
              value={formatVoltage(resolvePreTpsVinMv(status?.input))}
            />
            <MetricLine
              label="VIN IIN"
              value={formatCurrent(status?.input.vin_iin_ma)}
            />
            <MetricLine
              label="Pressure"
              value={`${pressureState} / ${pressureScore}%`}
            />
            <MetricLine
              label="Reason"
              value={pressureReasonLabel}
            />
            <MetricLine
              label="TPS output"
              value={powerThresholdSummary(tpsTotalIoutMa, tpsLimitThresholdMa)}
            />
            <MetricLine
              label="VIN baseline"
              value={formatVoltage(status?.input.vin_baseline_mv)}
            />
            <MetricLine
              label="VIN drop"
              value={formatVoltage(status?.input.vin_drop_mv)}
            />
          </InfoPanel>
          <InfoPanel title="Charger" icon={BatteryCharging}>
            <MetricLine label="State" value={status?.charger.state ?? "--"} />
            <div className="power-limit-summary">
              <span className={`severity-badge severity-${pressureSeverity}`}>
                {status?.charger.limit_active ? "limited" : "tracking"}
              </span>
              <strong>{formatCurrent(status?.charger.policy_target_ichg_ma)}</strong>
              <span>{limitReasonLabel}</span>
            </div>
            <MetricLine
              label="Stop cause"
              value={stopSummary}
            />
            <MetricLine
              label="Detail"
              value={status?.charger.detail_status ?? "--"}
            />
            <MetricLine
              label="Allow charge"
              value={boolLabel(status?.charger.allow_charge, "yes", "no")}
            />
            <MetricLine
              label="ICHG"
              value={formatCurrent(status?.charger.ichg_ma)}
            />
            <MetricLine
              label="Policy target"
              value={formatCurrent(status?.charger.policy_target_ichg_ma)}
            />
            <MetricLine
              label="Limit"
              value={boolLabel(status?.charger.limit_active, "yes", "no")}
            />
            <MetricLine
              label="Limit reason"
              value={limitReasonLabel}
            />
            <MetricLine
              label="Limit threshold"
              value={formatCurrent(status?.charger.limit_threshold_ma)}
            />
            <MetricLine
              label="IBAT"
              value={formatCurrent(status?.charger.ibat_ma)}
            />
          </InfoPanel>
        </div>
      </section>

      <section className="power-domain-section" data-evidence-target="power-discharging">
        <div className="power-domain-header">
          <span className="power-domain-icon">
            <Cable size={18} />
          </span>
          <div>
            <h2>Discharging</h2>
            <p>
              Output gate state and live load on each UPS output path.
            </p>
          </div>
        </div>

        <div className="detail-grid charge-diagnostics-grid">
          <InfoPanel
            title="Output gate"
            icon={Cable}
            className="charge-diagnostics-span-two"
          >
            <MetricLine
              label="Requested"
              value={status?.output.requested ?? "--"}
            />
            <MetricLine label="Active" value={status?.output.active ?? "--"} />
            <MetricLine
              label="Recoverable"
              value={status?.output.recoverable ?? "--"}
            />
            <MetricLine
              label="Gate reason"
              value={status?.output.gate_reason ?? "none"}
            />
          </InfoPanel>
          <InfoPanel title="OUT A" icon={Activity}>
            <MetricLine
              label="State"
              value={status?.output.out_a.state ?? "--"}
            />
            <MetricLine
              label="Enabled"
              value={boolLabel(status?.output.out_a.enabled, "yes", "no")}
            />
            <MetricLine
              label="Voltage"
              value={formatVoltage(status?.output.out_a.vbus_mv)}
            />
            <MetricLine
              label="Current"
              value={formatCurrent(status?.output.out_a.iout_ma)}
            />
          </InfoPanel>
          <InfoPanel title="OUT B" icon={Activity}>
            <MetricLine
              label="State"
              value={status?.output.out_b.state ?? "--"}
            />
            <MetricLine
              label="Enabled"
              value={boolLabel(status?.output.out_b.enabled, "yes", "no")}
            />
            <MetricLine
              label="Voltage"
              value={formatVoltage(status?.output.out_b.vbus_mv)}
            />
            <MetricLine
              label="Current"
              value={formatCurrent(status?.output.out_b.iout_ma)}
            />
          </InfoPanel>
        </div>
      </section>
      <Dialog.Root
        open={requestDialogOpen}
        onOpenChange={(open) => {
          if (busy !== null) return;
          setRequestDialogOpen(open);
        }}
      >
        <Dialog.Portal>
          <Dialog.Overlay className="pwa-update-dialog-overlay" />
          <Dialog.Content className="pwa-update-dialog charge-request-dialog">
            <Dialog.Title className="pwa-update-dialog-title">
              Request manual charge
            </Dialog.Title>
            <Dialog.Description className="pwa-update-dialog-description">
              Choose the target, charge current, timer, and preferred power
              path. The preview and every explanation below come from the
              device's formal charge-control contract.
            </Dialog.Description>
            <div className="settings-form">
              <SettingsSegmentedControl
                label="Charge target"
                value={manualPrefs.target}
                options={[
                  ["pack_3v7", "3.7V"],
                  ["rsoc_80", "80%"],
                  ["full_100", "100%"],
                ]}
                onChange={(target) =>
                  setManualPrefs((current) => ({
                    ...current,
                    target: target as DeviceSettings["manual_charge"]["target"],
                  }))
                }
              />
              <SettingsSegmentedControl
                label="Charge speed"
                value={manualPrefs.speed}
                options={[
                  ["ma_100", "100mA"],
                  ["ma_500", "500mA"],
                  ["ma_1000", "1A"],
                ]}
                onChange={(speed) =>
                  setManualPrefs((current) => ({
                    ...current,
                    speed: speed as DeviceSettings["manual_charge"]["speed"],
                  }))
                }
              />
              <SettingsSegmentedControl
                label="Timer"
                value={String(manualPrefs.timer_h)}
                options={[
                  ["1", "1h"],
                  ["2", "2h"],
                  ["6", "6h"],
                ]}
                onChange={(timer) =>
                  setManualPrefs((current) => ({
                    ...current,
                    timer_h: Number(timer) as 1 | 2 | 6,
                  }))
                }
              />
              <SettingsSegmentedControl
                label="Power path"
                value={manualPrefs.power_path ?? "auto"}
                options={[
                  ["auto", "Auto"],
                  ["dcin", "DCIN"],
                  ["usbc", "USB-C"],
                ]}
                onChange={(powerPath) =>
                  setManualPrefs((current) => ({
                    ...current,
                    power_path: powerPath,
                  }))
                }
              />
              <div className="settings-copy">
                <p className="field-help">
                  Next preset: {manualChargePresetLabel(manualPrefs)} via{" "}
                  {chargePowerPathLabel(manualPrefs.power_path ?? "auto")}.
                </p>
                <p className="field-help">
                  Planned path: {chargeControlPathLabel(requestDetail)}.
                </p>
                <p className="field-help">
                  {previewBusy
                    ? "Loading direct readiness evidence from the device..."
                    : chargeControlSummaryText(requestDetail)}
                </p>
                {requestDetail?.readiness.state === "confirm_required" ? (
                  <p className="field-help">
                    Granting override only bypasses:{" "}
                    {requestDetail.readiness.loop_override.allowed_guards.join(
                      ", ",
                    )}
                    . Current output power is{" "}
                    {requestDetail.telemetry.power_telemetry_fresh === false ||
                    requestDetail.telemetry.output_power_w10 === null
                      ? "unknown"
                      : formatPowerWatts(requestDetail.telemetry.output_power_w10)}
                    .
                  </p>
                ) : null}
                {chargeControlEvidenceText(requestDetail) ? (
                  <p className="field-help">
                    Direct evidence: {chargeControlEvidenceText(requestDetail)}
                  </p>
                ) : null}
              </div>
              <details className="settings-copy">
                <summary>Charge capabilities and rules</summary>
                <p className="field-help">
                  Target voltage:{" "}
                  {formatVoltage(settings?.charge_capabilities?.target_voltage_mv)}
                  . Normal / derated current:{" "}
                  {formatCurrent(settings?.charge_capabilities?.normal_current_ma)} /{" "}
                  {formatCurrent(
                    settings?.charge_capabilities?.dc_derated_current_ma,
                  )}
                  . DCIN limit:{" "}
                  {formatCurrent(settings?.charge_capabilities?.dcin_input_limit_ma)}.
                </p>
                <p className="field-help">
                  USB-C PD gate:{" "}
                  {settings?.charge_capabilities
                    ? `${(settings.charge_capabilities.usb_pd_high_power_min_voltage_mv / 1000).toFixed(2)} V-${(settings.charge_capabilities.usb_pd_high_power_max_voltage_mv / 1000).toFixed(2)} V / ${(settings.charge_capabilities.usb_pd_high_power_min_power_mw / 1000).toFixed(0)} W`
                    : "--"}
                  . Loop-free start:{" "}
                  {settings?.charge_capabilities
                    ? `< ${(settings.charge_capabilities.loop_start_max_power_without_confirm_w10 / 10).toFixed(1)} W`
                    : "--"}
                  . Loop stop latch:{" "}
                  {settings?.charge_capabilities
                    ? `> ${(settings.charge_capabilities.loop_stop_power_latched_w10 / 10).toFixed(1)} W`
                    : "--"}
                  .
                </p>
              </details>
              {dialogFeedback ? <FeedbackMessage feedback={dialogFeedback} /> : null}
            </div>
            <div className="pwa-update-dialog-actions">
              <Dialog.Close asChild>
                <button
                  className="secondary-button"
                  type="button"
                  disabled={busy !== null}
                >
                  Cancel
                </button>
              </Dialog.Close>
              {liveDetail?.summary.manual_active ? (
                <button
                  className="secondary-button"
                  type="button"
                  disabled={busy !== null}
                  onClick={() => void onRequestDialogStop()}
                >
                  <ButtonLabel
                    busy={busy === "stop"}
                    busyText="Stopping"
                    text="STOP"
                  />
                </button>
              ) : null}
              <button
                className="secondary-button"
                type="button"
                disabled={busy !== null}
                onClick={() => void onRequestDialogSaveDefaults()}
              >
                <ButtonLabel
                  busy={busy === "request-save"}
                  busyText="Saving"
                  text="Save defaults"
                />
              </button>
              <button
                className="primary-button"
                type="button"
                disabled={
                  busy !== null ||
                  previewBusy ||
                  requestDetail?.readiness.action === "none" ||
                  liveDetail?.summary.manual_active === true
                }
                onClick={() =>
                  void onRequestDialogStart(
                    requestDetail?.readiness.state === "confirm_required",
                  )
                }
              >
                <ButtonLabel
                  busy={busy === "request-start" || busy === "confirm"}
                  busyText={
                    busy === "confirm" ? "Confirming" : "Requesting"
                  }
                  text={
                    requestDetail?.readiness.state === "confirm_required"
                      ? "Allow and START"
                      : "Request START"
                  }
                />
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </section>
  );
}

function defaultManualChargePrefs(): DeviceSettings["manual_charge"] {
  return {
    target: "full_100",
    speed: "ma_500",
    timer_h: 2,
    power_path: "auto",
  };
}

function manualChargePresetLabel(
  prefs: DeviceSettings["manual_charge"],
): string {
  return [
    manualChargeTargetLabel(prefs.target),
    manualChargeSpeedLabel(prefs.speed),
    manualChargeTimerLabel(prefs.timer_h),
  ].join(" / ");
}

function manualChargeTargetLabel(target: string): string {
  if (target === "pack_3v7") return "3.7V";
  if (target === "rsoc_80") return "80%";
  if (target === "full_100") return "100%";
  return target;
}

function manualChargeSpeedLabel(speed: string): string {
  if (speed === "ma_100") return "100mA";
  if (speed === "ma_500") return "500mA";
  if (speed === "ma_1000") return "1A";
  return speed;
}

function manualChargeTimerLabel(timer: number): string {
  return `${timer}h`;
}

function chargeModeLabel(detail: ChargeControlDetail | null | undefined): string {
  if (!detail) return "SYNC";
  if (detail.summary.manual_active) return "MANUAL";
  if (detail.summary.stop_inhibit) return "HOLD";
  return "AUTO";
}

function chargeControlSeverity(
  detail: ChargeControlDetail | null | undefined,
): "ok" | "info" | "warning" {
  if (!detail) return "info";
  if (detail.readiness.state === "blocked") return "warning";
  if (detail.readiness.state === "confirm_required") return "warning";
  if (detail.summary.manual_active && detail.summary.loop_override_active)
    return "warning";
  if (detail.summary.manual_active) return "info";
  if (detail.summary.stop_inhibit) return "warning";
  return "ok";
}

function chargeControlHeadline(
  detail: ChargeControlDetail | null | undefined,
): string {
  if (!detail) return "Loading charge control details";
  switch (detail.readiness.state) {
    case "running":
      return "Manual charge session active";
    case "blocked":
      return "Manual charge is not ready";
    case "confirm_required":
      return "USB-C loop confirmation required";
    case "ready":
      return "Manual charge is ready to start";
    default:
      return "Automatic charging state";
  }
}

function chargeControlSummaryText(
  detail: ChargeControlDetail | null | undefined,
): string {
  if (!detail) {
    return "The app is fetching direct readiness evidence from the device.";
  }
  const plannedPath = chargeControlPathLabel(detail);
  if (detail.summary.manual_active) {
    return `Following ${plannedPath} with ${formatCurrent(
      detail.telemetry.policy_target_ichg_ma,
    )} target and ${formatCurrent(detail.telemetry.ibat_actual_ma)} actual battery current.`;
  }
  if (detail.readiness.state === "blocked" && detail.readiness.block) {
    return `${sentenceWithTerminator(detail.readiness.block.message)} Planned path: ${plannedPath}.`;
  }
  if (detail.readiness.state === "confirm_required") {
    return `START would bind ${plannedPath}. Current output power is ${
      detail.telemetry.power_telemetry_fresh === false ||
      detail.telemetry.output_power_w10 === null
        ? "unknown"
        : formatPowerWatts(detail.telemetry.output_power_w10)
    }, so USB-C loop confirmation is required.`;
  }
  if (detail.summary.stop_inhibit) {
    return `Manual charge is held after ${powerReasonLabel(
      detail.summary.last_stop_reason,
    )}. A new request is required before it can resume.`;
  }
  return `Next START will bind ${plannedPath} and target ${formatCurrent(
    detail.telemetry.policy_target_ichg_ma,
  )}.`;
}

function chargeControlReadinessHint(
  detail: ChargeControlDetail | null | undefined,
): string {
  if (!detail) {
    return "Waiting for the device to return the current charge-control detail.";
  }
  if (detail.summary.manual_active) {
    return "Manual charge is already running on the bound path. Use the action area to inspect or stop the current session.";
  }
  if (detail.readiness.state === "confirm_required") {
    return "This request needs an explicit USB-C loopback override before the device will allow manual charge to start.";
  }
  if (detail.readiness.state === "blocked") {
    return "The selected preset is blocked right now. Clear the live block below, then request a new manual session.";
  }
  if (detail.summary.stop_inhibit) {
    return "A previous manual session is still held. Wait for the hold condition to clear, then request a fresh session.";
  }
  return "The current preset is startable now and the device is ready to accept a manual charge request.";
}

function chargeControlActionHint(
  detail: ChargeControlDetail | null | undefined,
): string {
  if (!detail) return "Charge-control actions are loading from the device.";
  if (detail.summary.manual_active) {
    return "Open the dialog to stop this session or update its defaults.";
  }
  if (detail.readiness.state === "confirm_required") {
    return "Open the request dialog and confirm the temporary USB-C loop override.";
  }
  if (detail.readiness.state === "blocked") {
    return "Open the request dialog after the current block clears.";
  }
  return "Open the request dialog to review the preset and start manual charge.";
}

function chargeControlLoopLabel(
  detail: ChargeControlDetail | null | undefined,
): string {
  if (!detail) return "--";
  if (detail.readiness.state === "confirm_required")
    return "confirmation required";
  if (detail.summary.loop_override_active) return "override active";
  if (detail.summary.manual_active && detail.readiness.planned_path.bound === "usbc") {
    return detail.telemetry.power_telemetry_fresh
      ? "USB-C guarded"
      : "USB-C telemetry stale";
  }
  if (detail.readiness.planned_path.bound === "dcin") return "not applicable on DCIN";
  return "inactive";
}

function chargeControlPathLabel(
  detail: ChargeControlDetail | null | undefined,
): string {
  if (!detail) return "--";
  return chargePowerPathLabel(
    detail.readiness.planned_path.bound ??
      detail.readiness.planned_path.requested,
  );
}

function chargePowerPathLabel(path: string | null | undefined): string {
  if (!path) return "--";
  if (path === "dcin") return "DCIN";
  if (path === "usbc") return "USB-C";
  if (path === "auto") return "Auto";
  if (path === "usbc_pd_high_power") return "USB-C high-power PD";
  return path.replaceAll("_", " ");
}

function formatRemainingMinutes(value: number | null | undefined): string {
  if (typeof value !== "number") return "--";
  if (value <= 0) return "0m";
  const hours = Math.floor(value / 60);
  const minutes = value % 60;
  if (hours === 0) return `${minutes}m`;
  if (minutes === 0) return `${hours}h`;
  return `${hours}h ${minutes}m`;
}

function formatPowerWatts(valueW10: number | null | undefined): string {
  if (typeof valueW10 !== "number") return "--";
  return `${(valueW10 / 10).toFixed(1)} W`;
}

function sentenceWithTerminator(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  return /[.!?]$/.test(trimmed) ? trimmed : `${trimmed}.`;
}

function chargeControlDetailFromPayload(
  details: unknown,
): ChargeControlDetail | null {
  if (!details || typeof details !== "object") return null;
  if (
    "summary" in details &&
    "readiness" in details &&
    "telemetry" in details &&
    "evidence" in details
  ) {
    return details as ChargeControlDetail;
  }
  return null;
}

function chargeControlEvidenceText(
  detail: ChargeControlDetail | null | undefined,
): string | null {
  if (!detail || detail.evidence.length === 0) return null;
  return chargeControlEvidenceEntries(detail)
    .slice(0, 3)
    .map((entry) => `${entry.label}: ${chargeControlEvidenceValue(entry.value)}`)
    .join(" · ");
}

function chargeControlEvidenceEntries(
  detail: ChargeControlDetail | null | undefined,
): ChargeControlDetail["evidence"] {
  if (!detail) return [];
  return detail.evidence.slice(0, 4);
}

function chargeControlEvidenceValue(
  value: ChargeControlDetail["evidence"][number]["value"],
): string {
  if (typeof value === "boolean") return value ? "yes" : "no";
  if (typeof value === "number") return String(value);
  if (typeof value === "string") return value;
  return "--";
}

function BatteryPage({ record }: { record: DeviceRecord }) {
  const battery = record.status?.battery;
  const cells = normalizeCellVoltages(battery?.cell_mv);
  const cellModel = buildCellBalanceModel(cells, battery);
  return (
    <section className="page-flow">
      <div className="detail-grid">
        <InfoPanel title="Pack status" icon={BatteryCharging}>
          <MetricLine label="State" value={battery?.state ?? "--"} />
          <MetricLine label="SOC" value={formatPercent(battery?.soc_pct)} />
          <MetricLine
            label="Pack voltage"
            value={formatVoltage(battery?.pack_mv)}
          />
          <MetricLine
            label="Current"
            value={formatCurrent(battery?.current_ma)}
          />
        </InfoPanel>
        <InfoPanel title="Cell voltages" icon={Activity}>
          <div className="battery-balance-summary">
            <span
              className={`balance-delta balance-delta-${cellModel.severity}`}
            >
              Delta {formatMillivolts(cellModel.deltaMv)}
            </span>
            <span>{cellModel.balanceLabel}</span>
            <span>Start {formatMillivolts(cellModel.startDeltaMv)}</span>
          </div>
          <div className="battery-cell-grid" aria-label="BMS cell voltages">
            {cellModel.cells.map((cell, index) => (
              <div
                className={`battery-cell-tile battery-cell-${cell.severity}`}
                key={index}
              >
                <span>
                  C{index + 1}
                  {cell.isBalancing ? <em>BAL</em> : null}
                </span>
                <strong>{formatVoltage(cell.value)}</strong>
                <small>{formatCellOffset(cell.offsetMv)}</small>
              </div>
            ))}
          </div>
        </InfoPanel>
        <InfoPanel title="BMS readiness" icon={Cpu}>
          <MetricLine
            label="No battery"
            value={boolLabel(battery?.no_battery, "yes", "no")}
          />
          <MetricLine
            label="Discharge ready"
            value={boolLabel(battery?.discharge_ready, "yes", "no")}
          />
          <MetricLine
            label="Recovery pending"
            value={boolLabel(battery?.recovery_pending, "yes", "no")}
          />
          <MetricLine
            label="Last result"
            value={battery?.last_result ?? "--"}
          />
        </InfoPanel>
        <InfoPanel title="BMS MOS" icon={Cpu}>
          <MetricLine
            label="CHG MOS"
            value={fetLabel(battery?.charge_fet_on)}
          />
          <MetricLine
            label="DSG MOS"
            value={fetLabel(battery?.discharge_fet_on)}
          />
          <MetricLine
            label="PCHG MOS"
            value={fetLabel(battery?.precharge_fet_on)}
          />
        </InfoPanel>
        <InfoPanel title="Issue detail" icon={AlertTriangle}>
          <p className="panel-note">
            {battery?.issue_detail
              ? batteryIssueDetailSummary(battery.issue_detail)
              : "No active battery issue reported by the v1 status snapshot."}
          </p>
        </InfoPanel>
      </div>
    </section>
  );
}

function normalizeCellVoltages(
  cells: Array<number | null> | null | undefined,
): Array<number | null> {
  return [0, 1, 2, 3].map((index) => cells?.[index] ?? null);
}

type CellBalanceModel = {
  cells: Array<{
    value: number | null;
    offsetMv: number | null;
    severity: CellDeltaSeverity;
    isBalancing: boolean;
  }>;
  deltaMv: number | null;
  startDeltaMv: number | null;
  severity: CellDeltaSeverity;
  balanceLabel: string;
};

type CellDeltaSeverity = "unknown" | "ok" | "watch" | "warning" | "critical";

function buildCellBalanceModel(
  cells: Array<number | null>,
  battery: UpsStatus["battery"] | undefined,
): CellBalanceModel {
  const numericCells = cells.filter(
    (cell): cell is number => typeof cell === "number",
  );
  const minCell = numericCells.length > 0 ? Math.min(...numericCells) : null;
  const computedDelta =
    numericCells.length > 1
      ? Math.max(...numericCells) - Math.min(...numericCells)
      : null;
  const deltaMv = battery?.cell_delta_mv ?? computedDelta;
  const startDeltaMv =
    battery?.balance_min_start_delta_mv ??
    (battery?.balance_cfg_match === true ? 3 : null);
  const balanceMask = battery?.balance_mask ?? null;
  return {
    cells: cells.map((value, index) => {
      const offsetMv =
        value !== null && minCell !== null ? value - minCell : null;
      return {
        value,
        offsetMv,
        severity: cellDeltaSeverity(offsetMv, startDeltaMv),
        isBalancing: isBalanceCellActive(balanceMask, index),
      };
    }),
    deltaMv,
    startDeltaMv,
    severity: cellDeltaSeverity(deltaMv, startDeltaMv),
    balanceLabel: balanceStateLabel(battery),
  };
}

function cellDeltaSeverity(
  deltaMv: number | null,
  startDeltaMv: number | null,
): CellDeltaSeverity {
  if (deltaMv === null) return "unknown";
  const threshold = startDeltaMv ?? 3;
  if (deltaMv <= threshold) return "ok";
  if (deltaMv <= 25) return "watch";
  if (deltaMv <= 200) return "warning";
  return "critical";
}

function balanceStateLabel(battery: UpsStatus["battery"] | undefined): string {
  if (!battery) return "BAL --";
  if (battery.balance_enabled === false) return "BAL OFF";
  if (battery.balance_enabled === true && battery.balance_active === false)
    return "BAL IDLE";
  if (battery.balance_active === true) {
    if (typeof battery.balance_cell === "number")
      return `BAL C${battery.balance_cell}`;
    const mask = battery.balance_mask ?? 0;
    if (mask !== 0 && (mask & (mask - 1)) !== 0) return "BAL MULTI";
    return "BAL ACTIVE";
  }
  return "BAL --";
}

function isBalanceCellActive(mask: number | null, index: number): boolean {
  return mask !== null && (mask & (1 << index)) !== 0;
}

function formatMillivolts(value: number | null | undefined): string {
  return typeof value === "number" ? `${value} mV` : "--";
}

function formatCellOffset(value: number | null): string {
  if (value === null) return "--";
  return value === 0 ? "baseline" : `+${value} mV`;
}

function fetLabel(value: boolean | null | undefined): string {
  if (value === true) return "on";
  if (value === false) return "off";
  return "--";
}

function ThermalPage({ record }: { record: DeviceRecord }) {
  const thermal = record.status?.thermal;
  return (
    <section className="page-flow">
      <div className="detail-grid">
        <InfoPanel title="TMP A" icon={Thermometer}>
          <MetricLine label="State" value={thermal?.tmp_a_state ?? "--"} />
          <MetricLine
            label="Temperature"
            value={formatTemp(thermal?.tmp_a_c)}
          />
        </InfoPanel>
        <InfoPanel title="TMP B" icon={Thermometer}>
          <MetricLine label="State" value={thermal?.tmp_b_state ?? "--"} />
          <MetricLine
            label="Temperature"
            value={formatTemp(thermal?.tmp_b_c)}
          />
        </InfoPanel>
        <InfoPanel title="Protection context" icon={AlertTriangle}>
          <MetricLine
            label="Output gate"
            value={record.status?.output.gate_reason ?? "none"}
          />
          <MetricLine
            label="Charger"
            value={record.status?.charger.state ?? "--"}
          />
        </InfoPanel>
      </div>
    </section>
  );
}

function DeviceInfoPage({ record }: { record: DeviceRecord }) {
  const identity = record.identity;
  const network = record.network;
  const hardwareCapability = resolveUpsHardwareCapability(record);
  return (
    <section className="page-flow">
      <div className="detail-grid">
        <InfoPanel title="Identity" icon={Server}>
          <MetricLine
            label="Device ID"
            value={identity?.device_id ?? record.target.deviceId}
          />
          <MetricLine label="Hostname" value={identity?.hostname ?? "--"} />
          <MetricLine label="FQDN" value={identity?.hostname_fqdn ?? "--"} />
          <MetricLine label="API" value={identity?.api_version ?? "--"} />
        </InfoPanel>
        <InfoPanel title="Network" icon={Globe2}>
          <MetricLine label="State" value={network?.state ?? "--"} />
          <MetricLine label="IPv4" value={network?.ipv4 ?? "--"} />
          <MetricLine label="Gateway" value={network?.gateway ?? "--"} />
          <MetricLine
            label="RSSI"
            value={network?.rssi_dbm ? `${network.rssi_dbm} dBm` : "--"}
          />
        </InfoPanel>
        <InfoPanel title="Firmware" icon={Cpu}>
          <MetricLine
            label="Version"
            value={identity?.firmware.package_version ?? "--"}
          />
          <MetricLine
            label="Profile"
            value={identity?.firmware.build_profile ?? "--"}
          />
          <MetricLine
            label="Build"
            value={identity?.firmware.build_id ?? "--"}
          />
          <MetricLine label="Git" value={identity?.firmware.git_sha ?? "--"} />
        </InfoPanel>
        <InfoPanel title="Hardware capabilities" icon={PlugZap}>
          <MetricLine
            label="output_profile"
            value={hardwareCapabilityOutputProfileLabel(hardwareCapability)}
          />
          <MetricLine
            label="rated_vout_mv"
            value={ratedVoutMillivoltLabel(hardwareCapability.ratedVoutMv)}
          />
          <MetricLine
            label="Rated output"
            value={formatVoltage(hardwareCapability.ratedVoutMv)}
          />
          <MetricLine
            label="Source"
            value={hardwareCapabilitySourceLabel(hardwareCapability.source)}
          />
        </InfoPanel>
        <TpsEnableInterlockPanel record={record} />
      </div>
    </section>
  );
}

function TpsEnableInterlockPanel({ record }: { record: DeviceRecord }) {
  const target = useMemo(
    () => devdTpsEnableTarget(record),
    [
      record.serial?.baseUrl,
      record.serial?.leaseId,
      record.serial?.source,
      record.target.baseUrl,
      record.target.deviceId,
      record.target.mock,
      record.target.rememberedChannels?.devd?.baseUrl,
      record.target.rememberedChannels?.devd?.devdDeviceId,
      record.target.transport,
    ],
  );
  const [interlock, setInterlock] = useState<TpsEnableInterlock | null>(null);
  const [feedback, setFeedback] = useState<UiFeedback | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const targetKey = target
    ? `${target.baseUrl}\u0000${target.deviceId}\u0000${target.leaseId ?? ""}`
    : null;
  const currentTargetKey = useRef(targetKey);
  currentTargetKey.current = targetKey;

  const refresh = useCallback(async (isActive: () => boolean = () => true) => {
    const refreshTarget = target;
    const refreshTargetKey = targetKey;
    const isCurrent = () =>
      isActive() && currentTargetKey.current === refreshTargetKey;
    if (!refreshTarget || record.target.mock) {
      if (isCurrent()) {
        setInterlock(null);
        setFeedback(null);
      }
      return;
    }
    try {
      const snapshot = await getDevdDeviceDiagSnapshot(
        refreshTarget.baseUrl,
        refreshTarget.deviceId,
      );
      if (!isCurrent()) return;
      const next = snapshot.packages["mcu.runtime"]?.payload
        ?.tps_enable_interlock;
      if (next) {
        setInterlock(next);
        setFeedback(null);
      } else {
        setInterlock(null);
        setFeedback({
          tone: "error",
          message: "TPS enable interlock diagnostics are unavailable on this firmware.",
        });
      }
    } catch (error) {
      if (!isCurrent()) return;
      setInterlock(null);
      setFeedback(errorFeedback(toErrorEnvelope(error)));
    }
  }, [record.target.mock, target, targetKey]);

  useEffect(() => {
    let active = true;
    setInterlock(null);
    setFeedback(null);
    setDialogOpen(false);
    setBusy(false);
    void refresh(() => active);
    return () => {
      active = false;
    };
  }, [refresh]);

  async function release() {
    const releaseTarget = target;
    const releaseTargetKey = targetKey;
    if (!releaseTarget?.leaseId) return;
    setBusy(true);
    try {
      const result = await releaseDevdTpsEnableInterlock(
        releaseTarget.baseUrl,
        releaseTarget.deviceId,
        releaseTarget.leaseId,
      );
      if (currentTargetKey.current !== releaseTargetKey) return;
      setFeedback(
        result.warning
          ? {
              tone: "error",
              message:
                "MCU release completed, but THERM_KILL_N remains low from an external or unknown source.",
            }
          : successFeedback(
              result.result === "already_released"
                ? "TPS_EN was already released by the MCU"
                : "MCU TPS_EN hard inhibit released",
            ),
      );
      setDialogOpen(false);
      await refresh();
    } catch (error) {
      if (currentTargetKey.current !== releaseTargetKey) return;
      setFeedback(errorFeedback(toErrorEnvelope(error)));
    } finally {
      if (currentTargetKey.current === releaseTargetKey) setBusy(false);
    }
  }

  const leaseReady = Boolean(target?.leaseId);
  const source = interlock?.source ?? "unavailable";
  return (
    <InfoPanel title="TPS enable interlock" icon={AlertTriangle}>
      <MetricLine
        label="THERM_KILL_N"
        value={interlock ? (interlock.therm_kill_n_low ? "low" : "high") : "--"}
      />
      <MetricLine
        label="MCU drive"
        value={interlock ? (interlock.mcu_drive_low ? "low" : "released") : "--"}
      />
      <MetricLine
        label="TPS_EN inhibit"
        value={
          interlock
            ? interlock.tps_en_effective_inhibit
              ? "active"
              : "released"
            : "--"
        }
      />
      <MetricLine label="Source" value={source} />
      <MetricLine
        label="Asserted uptime"
        value={interlock?.asserted_at_ms === null || interlock?.asserted_at_ms === undefined ? "--" : `${interlock.asserted_at_ms} ms`}
      />
      <MetricLine
        label="Last release uptime"
        value={interlock?.last_release_at_ms === null || interlock?.last_release_at_ms === undefined ? "--" : `${interlock.last_release_at_ms} ms`}
      />
      {interlock?.failure_channel ? (
        <MetricLine
          label="Last TPS failure"
          value={`${interlock.failure_channel} ${interlock.failure_stage ?? "--"} ${interlock.failure_code ?? "--"}`}
        />
      ) : null}
      <div className="tps-enable-interlock-actions">
        <button
          className="secondary-button danger-action tps-enable-interlock-release"
          type="button"
          disabled={!leaseReady || busy || !interlock?.mcu_drive_low}
          onClick={() => setDialogOpen(true)}
        >
          <ButtonLabel
            icon={RefreshCw}
            busy={false}
            busyText="Releasing"
            text="Release MCU TPS_EN"
          />
        </button>
        {!leaseReady ? <p className="field-help">An active USB lease is required.</p> : null}
      </div>
      {feedback ? <FeedbackMessage feedback={feedback} /> : null}
      <Dialog.Root open={dialogOpen} onOpenChange={setDialogOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="pwa-update-dialog-overlay" />
          <Dialog.Content className="pwa-update-dialog">
            <Dialog.Title className="pwa-update-dialog-title">
              Release MCU TPS_EN inhibit
            </Dialog.Title>
            <Dialog.Description className="pwa-update-dialog-description">
              This only releases the MCU open-drain hold. It does not clear the TPS fault latch or restore output.
            </Dialog.Description>
            <div className="dialog-actions">
              <Dialog.Close asChild>
                <button className="secondary-button" type="button" disabled={busy}>
                  Cancel
                </button>
              </Dialog.Close>
              <button
                className="primary-button"
                type="button"
                disabled={busy}
                onClick={() => void release()}
              >
                <ButtonLabel
                  icon={RefreshCw}
                  busy={busy}
                  busyText="Releasing"
                  text="Release TPS_EN"
                />
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </InfoPanel>
  );
}

function devdTpsEnableTarget(record: DeviceRecord): {
  baseUrl: string;
  deviceId: string;
  leaseId: string | null;
} | null {
  const baseUrl =
    record.serial?.source === "devd"
      ? (record.serial.baseUrl ?? record.target.baseUrl)
      : record.target.transport === "devd"
        ? record.target.baseUrl
        : record.target.rememberedChannels?.devd?.baseUrl;
  if (baseUrl === undefined) return null;
  return {
    baseUrl,
    deviceId:
      record.target.rememberedChannels?.devd?.devdDeviceId ?? record.target.deviceId,
    leaseId:
      record.serial?.source === "devd" ? (record.serial.leaseId ?? null) : null,
  };
}

function SettingsPage({ record }: { record: DeviceRecord }) {
  const {
    sendWifiConfig,
    clearWifiConfig,
    setAdvancedPower,
    resetAdvancedPower,
  } =
    useDeviceRegistry();
  const settings = record.settings;
  const [ssid, setSsid] = useState(settings?.wifi.ssid ?? "");
  const [psk, setPsk] = useState("");
  const [advancedPower, setAdvancedPowerDraft] = useState<AdvancedPowerSettings>(
    settings?.advanced_power ?? defaultAdvancedPowerSettings(),
  );
  const [message, setMessage] = useState<UiFeedback | null>(null);
  const [wifiMessage, setWifiMessage] = useState<UiFeedback | null>(null);
  const [wifiProgress, setWifiProgress] =
    useState<WifiProvisioningProgress | null>(null);
  const [busy, setBusy] = useState<
    "wifi-save" | "wifi-clear" | "advanced-power" | "advanced-power-reset" | null
  >(null);
  const activeTransport = activeRecordTransport(record);
  const hardwareCapability = resolveUpsHardwareCapability(record);
  const settingsReady = deviceSettingsAvailable(record);
  const transportLabel =
    activeTransport === "http" ? "LAN" : activeTransport === "devd" ? "devd" : "hardware";
  const wifiValidationMessage = !ssid.trim()
    ? "Save requires an SSID."
    : psk.length < 8
      ? "Save requires an 8-63 character PSK."
      : null;
  const wifiDescribedBy = [
    "wifi-form-help",
    wifiValidationMessage ? "wifi-validation-help" : null,
    wifiProgress || wifiMessage ? "wifi-provisioning-message" : null,
  ]
    .filter(Boolean)
    .join(" ");

  useEffect(() => {
    if (!settings) return;
    setAdvancedPowerDraft(settings.advanced_power);
    if (settings.wifi.ssid) setSsid(settings.wifi.ssid);
  }, [settings]);

  useEffect(() => {
    if (
      (busy !== "wifi-save" && busy !== "wifi-clear") ||
      !record.status?.network
    )
      return;
    setWifiProgress(
      wifiProgressFromStatusNetwork(record.status.network, busy, ssid),
    );
  }, [busy, record.status?.network, ssid]);

  async function onWifiSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy("wifi-save");
    setMessage(null);
    setWifiMessage(null);
    setWifiProgress({
      phase: "saving",
      message: "Writing WiFi credentials to hardware",
    });
    const result = await sendWifiConfig(
      record.target.deviceId,
      { ssid, psk },
      setWifiProgress,
    );
    setBusy(null);
    setPsk("");
    setWifiProgress(null);
    setWifiMessage(
      result.ok
        ? successFeedback(result.message ?? `WiFi connected to ${ssid}`)
        : errorFeedback(result.error),
    );
  }

  async function onWifiClear() {
    setBusy("wifi-clear");
    setMessage(null);
    setWifiMessage(null);
    setWifiProgress({
      phase: "clearing",
      message: "Clearing WiFi credentials from hardware",
    });
    const result = await clearWifiConfig(
      record.target.deviceId,
      setWifiProgress,
    );
    setBusy(null);
    setWifiProgress(null);
    if (result.ok) {
      setSsid("");
      setPsk("");
      setWifiMessage(
        successFeedback(
          result.message ?? "WiFi credentials cleared and WiFi disconnected",
        ),
      );
    } else {
      setWifiMessage(errorFeedback(result.error));
    }
  }

  async function onAdvancedPowerSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy("advanced-power");
    setMessage(null);
    const result = await setAdvancedPower(record.target.deviceId, advancedPower);
    setBusy(null);
    setMessage(
      result.ok
        ? successFeedback("Advanced power settings updated")
        : errorFeedback(result.error),
    );
  }

  async function onAdvancedPowerReset() {
    setBusy("advanced-power-reset");
    setMessage(null);
    const result = await resetAdvancedPower(record.target.deviceId);
    setBusy(null);
    setMessage(
      result.ok
        ? successFeedback("Advanced power settings reset")
        : errorFeedback(result.error),
    );
  }

  if (!settingsReady) {
    return (
      <section className="page-flow">
        <section className="empty-state">
          <SlidersHorizontal size={28} />
          <h2>Settings unavailable</h2>
          <p>
            This connected device does not expose the settings contract yet.
            Refresh it after upgrading firmware or use a settings-capable
            transport before changing settings.
          </p>
          <button
            className="primary-button"
            type="button"
            onClick={() => navigate("/connect")}
          >
            Add device
          </button>
        </section>
      </section>
    );
  }

  return (
    <section className="page-flow" data-evidence-target="wifi-settings">
      <div className="settings-layout settings-layout-advanced">
        <section className="info-panel settings-panel">
          <header>
            <Wifi size={18} />
            <h2>WiFi provisioning</h2>
          </header>
          <form className="settings-form" onSubmit={onWifiSubmit}>
            <label>
              SSID
              <input
                name="wifi-ssid"
                value={ssid}
                onChange={(event) => setSsid(event.target.value)}
                maxLength={32}
                aria-describedby={wifiDescribedBy}
                aria-invalid={!ssid.trim()}
                required
              />
            </label>
            <label>
              PSK
              <input
                name="wifi-psk"
                value={psk}
                onChange={(event) => setPsk(event.target.value)}
                type="password"
                minLength={8}
                maxLength={63}
                autoComplete="new-password"
                aria-describedby={wifiDescribedBy}
                aria-invalid={psk.length < 8}
                required
              />
            </label>
            <div id="wifi-form-help" className="secret-note">
              <KeyRound size={15} /> PSK is written over {transportLabel} and
              cleared from the form after submit.
            </div>
            {wifiValidationMessage ? (
              <p id="wifi-validation-help" className="field-help">
                {wifiValidationMessage}
              </p>
            ) : null}
            <div className="form-actions wifi-actions">
              <span className="wifi-save-anchor">
                <button
                  className="primary-button"
                  type="submit"
                  disabled={busy !== null || wifiValidationMessage !== null}
                  aria-describedby={wifiDescribedBy}
                  aria-busy={busy === "wifi-save"}
                >
                  <ButtonLabel
                    busy={busy === "wifi-save"}
                    busyText="Saving"
                    text="Save WiFi"
                  />
                </button>
                {wifiProgress || wifiMessage ? (
                  <WifiProvisioningCallout
                    id="wifi-provisioning-message"
                    progress={wifiProgress}
                    feedback={wifiMessage}
                  />
                ) : null}
              </span>
              <button
                className="secondary-button"
                type="button"
                onClick={() => void onWifiClear()}
                disabled={busy !== null}
                aria-describedby={
                  wifiProgress || wifiMessage
                    ? "wifi-provisioning-message"
                    : undefined
                }
                aria-busy={busy === "wifi-clear"}
              >
                <ButtonLabel
                  icon={Trash2}
                  busy={busy === "wifi-clear"}
                  busyText="Clearing"
                  text="Clear"
                />
              </button>
            </div>
          </form>
        </section>

        <section className="info-panel settings-panel">
          <header>
            <SlidersHorizontal size={18} />
            <h2>Charge control moved</h2>
          </header>
          <div className="settings-copy">
            <p className="field-help">
              Manual and automatic charging controls now live on the Power page
              so the operator can see runtime path binding, limits, loop
              confirmation, and live charger telemetry before opening the
              manual-charge request dialog.
            </p>
            <div className="secret-note capability-note">
              <CircleHelp size={15} />
              <div>
                <strong>Use Power for charge control</strong>
                <span>
                  Settings only keeps WiFi and advanced power thresholds. Power
                  now keeps the live charge contract visible while the manual
                  charge request itself runs inside a popup flow.
                </span>
              </div>
            </div>
            <div className="form-actions">
              <button
                className="primary-button"
                type="button"
                onClick={() =>
                  navigate(deviceHref(record.target.deviceId, "power"))
                }
              >
                Open Power page
              </button>
            </div>
          </div>
        </section>

        <section className="info-panel settings-panel advanced-power-panel">
          <header>
            <AlertTriangle size={18} />
            <h2>Advanced Power</h2>
          </header>
          <form className="settings-form" onSubmit={onAdvancedPowerSubmit}>
            <div className="settings-copy">
              <p className="field-help">
                Tune the persisted thresholds that materially affect standby
                output, input UVLO, and source-limited backup takeover.
              </p>
              <div className="secret-note capability-note">
                <CircleHelp size={15} />
                <div>
                  <strong>{hardwareCapabilityHeadline(hardwareCapability)}</strong>
                  <span>{hardwareCapabilityDetail(hardwareCapability)}</span>
                </div>
              </div>
              {hardwareCapability.source === "settings" ? (
                <p className="field-help">
                  Hardware identity did not report capability fields yet, so this
                  profile is inferred from the current advanced-power schema.
                </p>
              ) : null}
              {hardwareCapability.source === "unknown" ? (
                <p className="field-help">
                  Hardware capability fields are not available yet. Refresh the
                  device before making 12V/19V-sensitive changes.
                </p>
              ) : null}
            </div>
            <AdvancedPowerField
              label="Standby drop"
              hint="How far below rated output the standby hot-standby target stays. Larger drop means less normal sharing."
              suffix="mV"
              value={advancedPower.standby_drop_mv}
              capability={settings?.advanced_power_capabilities.standby_drop_mv}
              onChange={(value) =>
                setAdvancedPowerDraft((current) => ({
                  ...current,
                  standby_drop_mv: value,
                }))
              }
            />
            <AdvancedPowerField
              label="Input UVLO cutoff"
              hint="Pre-TPS VIN below this level for the required sample count is treated as input absent, and MCU forces backup."
              suffix="mV"
              value={advancedPower.input_uvlo_cutoff_mv}
              capability={settings?.advanced_power_capabilities.input_uvlo_cutoff_mv}
              onChange={(value) =>
                setAdvancedPowerDraft((current) => ({
                  ...current,
                  input_uvlo_cutoff_mv: value,
                }))
              }
            />
            <AdvancedPowerField
              label="Input UVLO recover"
              hint="Pre-TPS VIN must recover above this level for the required sample count before MCU re-enables the input gate."
              suffix="mV"
              value={advancedPower.input_uvlo_recover_mv}
              capability={settings?.advanced_power_capabilities.input_uvlo_recover_mv}
              onChange={(value) =>
                setAdvancedPowerDraft((current) => ({
                  ...current,
                  input_uvlo_recover_mv: value,
                }))
              }
            />
            <AdvancedPowerField
              label="Input UVLO samples"
              hint="How many consecutive fresh samples the UVLO cutoff and recover decisions require."
              suffix="samples"
              value={advancedPower.input_uvlo_required_samples}
              capability={settings?.advanced_power_capabilities.input_uvlo_required_samples}
              onChange={(value) =>
                setAdvancedPowerDraft((current) => ({
                  ...current,
                  input_uvlo_required_samples: value,
                }))
              }
            />
            <AdvancedPowerField
              label="Source-limited enter delta"
              hint="How much extra TPS current must appear before MCU treats an online source as overloaded and takes over in backup mode."
              suffix="mA"
              value={advancedPower.source_limited_enter_delta_ma}
              capability={
                settings?.advanced_power_capabilities.source_limited_enter_delta_ma
              }
              onChange={(value) =>
                setAdvancedPowerDraft((current) => ({
                  ...current,
                  source_limited_enter_delta_ma: value,
                }))
              }
            />
            <div className="form-actions advanced-power-actions">
              <button
                className="primary-button"
                type="submit"
                disabled={busy !== null}
              >
                <ButtonLabel
                  busy={busy === "advanced-power"}
                  busyText="Applying"
                  text="Apply advanced power"
                />
              </button>
              <button
                className="secondary-button danger-action"
                type="button"
                disabled={busy !== null}
                onClick={() => void onAdvancedPowerReset()}
              >
                <ButtonLabel
                  icon={RefreshCw}
                  busy={busy === "advanced-power-reset"}
                  busyText="Resetting"
                  text="Reset to device default"
                />
              </button>
            </div>
          </form>
        </section>
      </div>
      <UsbDeveloperConsole
        logs={record.serial?.logs ?? []}
        trace={record.serial?.trace ?? []}
      />
      {message ? (
        <div className="command-feedback">
          {message.tone === "error" ? (
            <ConnectionCallout
              id="settings-command-message"
              message={message.message}
            />
          ) : (
            <FeedbackMessage feedback={message} />
          )}
        </div>
      ) : null}
    </section>
  );
}

function defaultAdvancedPowerSettings(): AdvancedPowerSettings {
  return buildAdvancedPowerDefaults(12_000);
}

function AdvancedPowerField({
  label,
  hint,
  suffix,
  value,
  capability,
  onChange,
}: {
  label: string;
  hint: string;
  suffix: string;
  value: number;
  capability:
    | {
        default: number;
        min: number;
        max: number;
        step: number;
      }
    | undefined;
  onChange: (value: number) => void;
}) {
  return (
    <label className="advanced-power-field">
      <span className="advanced-power-header">
        <strong>{label}</strong>
        <span>{suffix}</span>
      </span>
      <input
        type="number"
        inputMode="numeric"
        value={value}
        min={capability?.min}
        max={capability?.max}
        step={capability?.step}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <span className="field-help">{hint}</span>
      <span className="advanced-power-meta">
        Default {capability?.default ?? value} {suffix} · Range{" "}
        {capability?.min ?? value}..{capability?.max ?? value} · Step{" "}
        {capability?.step ?? 1}
      </span>
    </label>
  );
}

function wifiProgressFromStatusNetwork(
  network: UpsStatus["network"],
  busy: "wifi-save" | "wifi-clear",
  ssid: string,
): WifiProvisioningProgress {
  if (busy === "wifi-clear") {
    return network.state === "disabled"
      ? {
          phase: "disabled",
          message: "WiFi credentials cleared and WiFi disconnected",
          network,
        }
      : {
          phase: "clearing",
          message: "Disconnecting WiFi and clearing runtime credentials",
          network,
        };
  }
  if (network.state === "connected") {
    return network.ipv4
      ? {
          phase: "connected",
          message: `WiFi connected to ${ssid} at ${network.ipv4}`,
          network,
        }
      : {
          phase: "ip",
          message: "WiFi link is up. Waiting for an IP address",
          network,
        };
  }
  if (network.state === "connecting") {
    return {
      phase: "connecting",
      message: `Connecting to ${ssid} and waiting for an IP address`,
      network,
    };
  }
  return {
    phase: "starting",
    message: "Starting WiFi with the saved credentials",
    network,
  };
}

function ApiDebugPage({ record }: { record: DeviceRecord }) {
  const payload = {
    identity: record.identity,
    network: record.network,
    settings: record.settings,
    status: record.status,
    error: record.error,
    serial: record.serial
      ? { connected: record.serial.connected, protocol: record.serial.protocol }
      : null,
  };
  return (
    <section className="page-flow">
      <div className="api-layout">
        <InfoPanel title="Endpoints" icon={Cable}>
          <MetricLine label="Ping" value="/api/v1/ping" />
          <MetricLine label="Identity" value="/api/v1/identity" />
          <MetricLine label="Network" value="/api/v1/network" />
          <MetricLine label="Status" value="/api/v1/status" />
          <MetricLine label="SSE" value="Accept: text/event-stream" />
          <MetricLine
            label="USB CDC"
            value={record.serial?.connected ? "JSONL frames" : "not connected"}
          />
        </InfoPanel>
        <pre className="json-view">{JSON.stringify(payload, null, 2)}</pre>
      </div>
      {record.serial ? (
        <UsbDeveloperConsole
          logs={record.serial.logs}
          trace={record.serial.trace}
        />
      ) : null}
    </section>
  );
}

function DeviceStatusBand({ record }: { record: DeviceRecord }) {
  const status = record.status;
  const severity = deviceSeverity(record);
  const stream = streamPresentation(record);
  const hardwareCapability = resolveUpsHardwareCapability(record);
  return (
    <div className="status-band status-band-color-warm">
      <div className="live-cell">
        <span className="eyebrow">Operating state</span>
        <strong className="live-value">{modeLabel(status?.mode)}</strong>
      </div>
      <div className="live-cell">
        <span className="eyebrow">Output</span>
        <strong className="live-value">{status?.output.active ?? "--"}</strong>
      </div>
      <div className="live-cell">
        <span className="eyebrow">Battery</span>
        <strong className="live-value">
          {formatPercent(status?.battery.soc_pct)}
        </strong>
      </div>
      <div className="live-cell">
        <span className="eyebrow">Hardware</span>
        <strong className="live-value">
          {hardwareCapabilityHeadline(hardwareCapability)}
        </strong>
        <span className="live-detail">
          {hardwareCapabilityMetricDetail(hardwareCapability)}
        </span>
      </div>
      <div className="live-cell">
        <span className="eyebrow">Data</span>
        <strong className="live-value">{stream.label}</strong>
        <span className={`live-detail tone-${stream.tone}`}>
          {stream.detail}
        </span>
      </div>
      <span className={`severity-badge live-state severity-${severity}`}>
        {severity}
      </span>
    </div>
  );
}

function InfoPanel({
  title,
  icon: Icon,
  children,
  className,
}: {
  title: string;
  icon: typeof Gauge;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={className ? `info-panel ${className}` : "info-panel"}>
      <header>
        <Icon size={18} />
        <h2>{title}</h2>
      </header>
      {children}
    </section>
  );
}

function wait(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

type StreamPresentation = {
  label: string;
  detail: string;
  tone: "ok" | "info" | "warning" | "critical" | "offline";
};

function streamPresentation(record: DeviceRecord): StreamPresentation {
  const freshness = record.lastUpdated
    ? `, updated ${timeAgo(record.lastUpdated)}`
    : "";

  if (record.connectionState === "offline") {
    return {
      label: "Offline",
      detail: record.error?.message ?? "No recent device data",
      tone: "offline",
    };
  }

  if (record.connectionState === "connecting") {
    return {
      label: "Connecting",
      detail: record.status
        ? "Refreshing device data"
        : "Waiting for the first device response",
      tone: "info",
    };
  }

  if (record.connectionState === "error") {
    return {
      label: "Connection error",
      detail: record.error?.message ?? `Device data unavailable${freshness}`,
      tone: "critical",
    };
  }

  if (record.errorSource === "read") {
    return {
      label: "Data error",
      detail: record.error?.message ?? `Device data unavailable${freshness}`,
      tone: "warning",
    };
  }

  if (record.streamState === "error") {
    return {
      label: "Data degraded",
      detail: record.status
        ? `Transport reconnecting, polling fallback${freshness}`
        : `Stream error${freshness}`,
      tone: "warning",
    };
  }

  if (record.error) {
    return {
      label: "Action failed",
      detail: record.error.message,
      tone: "warning",
    };
  }

  if (record.streamState === "polling") {
    return {
      label: record.status ? "Live data" : "Waiting",
      detail: record.status
        ? `Polling fallback${freshness}`
        : `Polling for device data${freshness}`,
      tone: record.status ? "info" : "warning",
    };
  }

  if (record.streamState === "streaming" || record.streamState === "idle") {
    return {
      label: record.status ? "Live" : "Waiting",
      detail: record.status
        ? `${record.streamState}${freshness}`
        : `Waiting for the first device response${freshness}`,
      tone: record.status ? "ok" : "warning",
    };
  }

  return {
    label: "Unknown",
    detail: `${record.streamState}${freshness}`,
    tone: "info",
  };
}

function Metric({
  label,
  value,
  tone = "neutral",
  onClick,
}: {
  label: string;
  value: number;
  tone?: "neutral" | "critical" | "warning" | "offline" | "ok";
  onClick?: () => void;
}) {
  if (onClick) {
    return (
      <button
        className={`top-metric is-actionable tone-${tone}`}
        type="button"
        onClick={onClick}
        aria-label={`Open ${value} ${label.toLowerCase()} alerts`}
      >
        <span>{label}</span>
        <strong>{value}</strong>
      </button>
    );
  }
  return (
    <div className={`top-metric tone-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function MetricLine({
  label,
  value,
  title,
}: {
  label: string;
  value: string;
  title?: string;
}) {
  return (
    <div className="metric-line" title={title}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function SettingsSegmentedControl<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: Array<[T, string]>;
  onChange: (value: T) => void;
}) {
  return (
    <div className="control-row">
      <span>{label}</span>
      <SegmentedControl
        label={label}
        value={value}
        options={options}
        onChange={onChange}
        variant="compact"
      />
    </div>
  );
}

type TraceLevelFilter = "all" | "error" | "warn" | "info" | "debug" | "trace";
type TraceDirectionFilter = "all" | SerialTraceEntry["direction"];

const traceLevelRank: Record<Exclude<TraceLevelFilter, "all">, number> = {
  error: 0,
  warn: 1,
  info: 2,
  debug: 3,
  trace: 4,
};

function traceEntryLevel(
  entry: SerialTraceEntry,
): Exclude<TraceLevelFilter, "all"> {
  if (entry.frameType === "error") return "error";
  if (entry.kind === "ignored" && entry.frameType === "defmt") return "warn";
  if (entry.kind === "ignored") return "trace";
  const bracketLevel = entry.payload
    .match(/^\[(ERROR|WARN|INFO|DEBUG|TRACE)\s*\]/i)?.[1]
    ?.toLowerCase();
  if (bracketLevel && bracketLevel in traceLevelRank)
    return bracketLevel as Exclude<TraceLevelFilter, "all">;
  try {
    const parsed = JSON.parse(entry.payload) as { level?: unknown };
    if (typeof parsed.level === "string" && parsed.level in traceLevelRank)
      return parsed.level as Exclude<TraceLevelFilter, "all">;
  } catch {
    // Payloads can be plain boot logs or legacy console text.
  }
  if (entry.kind === "raw") return "debug";
  return "info";
}

function traceSearchText(entry: SerialTraceEntry, level: string) {
  return [
    entry.direction,
    level,
    entry.kind,
    entry.frameType,
    entry.requestId,
    entry.target,
    entry.summary,
    entry.payload,
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

type DefmtDecodeStatus = {
  label: string;
  tone: "ok" | "warn" | "muted";
  detail: string;
};

function isDefmtAwaitingDecoder(entry: SerialTraceEntry): boolean {
  if (entry.frameType !== "defmt") return false;
  return `${entry.summary} ${entry.payload}`
    .toLowerCase()
    .includes("awaiting decoder");
}

function defmtDecodeStatus(trace: SerialTraceEntry[]): DefmtDecodeStatus {
  const defmtEntries = trace.filter((entry) => entry.frameType === "defmt");
  const decoded = defmtEntries.filter(
    (entry) =>
      !isDefmtAwaitingDecoder(entry) &&
      entry.kind !== "ignored" &&
      entry.summary.trim().length > 0,
  );
  if (decoded.length > 0) {
    return {
      label: "defmt decoded",
      tone: "ok",
      detail:
        "defmt frames are being decoded with the current firmware metadata.",
    };
  }
  return {
    label: "defmt idle",
    tone: "muted",
    detail: "No binary defmt frames are present in the current trace window.",
  };
}

function TraceHelpBubble({ status }: { status: DefmtDecodeStatus }) {
  return (
    <span className={`trace-help-bubble decode-${status.tone}`}>
      <button
        type="button"
        className="trace-help-trigger"
        aria-label={`USB console help: ${status.label}`}
      >
        <CircleHelp size={15} strokeWidth={1.9} />
      </button>
      <span className="trace-help-popover" aria-hidden="true">
        <strong>{status.label}</strong>
        <span>{status.detail}</span>
        <span>
          Raw shows decoded defmt text when possible. Parsed hides original
          payloads. Compare shows both.
        </span>
      </span>
    </span>
  );
}

type ParsedTraceMessage = {
  lead: string;
  fields: Array<{ key: string; value: string }>;
};

function parseTraceMessage(message: string): ParsedTraceMessage {
  const fields: Array<{ key: string; value: string }> = [];
  const fieldPattern = /(?:^|\s)([A-Za-z][A-Za-z0-9_./-]*)=("[^"]*"|\S+)/g;
  let leadEnd = message.length;
  let match: RegExpExecArray | null;
  while ((match = fieldPattern.exec(message)) !== null) {
    if (fields.length === 0) leadEnd = match.index;
    fields.push({ key: match[1], value: match[2] });
  }
  const lead =
    message.slice(0, leadEnd).trim() ||
    message.split(/\s+/).slice(0, 2).join(" ");
  return { lead, fields };
}

function traceSummaryLabel(entry: SerialTraceEntry): string {
  if (entry.kind === "event" && entry.target === "power") return "power event";
  if (entry.kind !== "frame" && entry.frameType === "defmt")
    return parseTraceMessage(entry.summary).lead;
  return entry.summary;
}

function TraceMessage({
  entry,
  query,
  mode,
}: {
  entry: SerialTraceEntry;
  query: string;
  mode: "summary" | "raw";
}) {
  if (mode !== "raw" && entry.kind === "event" && entry.target === "power") {
    const payload = safeParseTracePayload(entry.payload);
    const actualMa = asNullableNumber(payload?.tps_total_iout_ma);
    const thresholdMa = asNullableNumber(payload?.tps_limit_threshold_ma);
    return (
      <div className="trace-message-readable">
        <p className="trace-message-lead">
          <HighlightText
            value={powerEventSummary(payload)}
            query={query}
          />
        </p>
        <dl className="trace-field-list">
          <div className="trace-field">
            <dt>source</dt>
            <dd>{String(payload?.input_source ?? "--")}</dd>
          </div>
          <div className="trace-field">
            <dt>score</dt>
            <dd>{String(payload?.pressure_score_pct ?? "--")}</dd>
          </div>
          <div className="trace-field">
            <dt>target</dt>
            <dd>{String(payload?.policy_target_ichg_ma ?? "--")}</dd>
          </div>
          <div className="trace-field">
            <dt>tps</dt>
            <dd>{powerThresholdSummary(actualMa, thresholdMa)}</dd>
          </div>
        </dl>
      </div>
    );
  }
  if (mode === "raw" || entry.kind === "frame" || entry.frameType !== "defmt") {
    return (
      <HighlightText
        value={mode === "raw" ? entry.payload : entry.summary}
        query={query}
      />
    );
  }
  const parsed = parseTraceMessage(entry.summary);
  return (
    <div className="trace-message-readable">
      <p className="trace-message-lead">
        <HighlightText value={parsed.lead} query={query} />
      </p>
      {parsed.fields.length > 0 ? (
        <dl className="trace-field-list">
          {parsed.fields.map((field, index) => (
            <div
              className="trace-field"
              key={`${entry.id}-${field.key}-${index}`}
            >
              <dt>
                <HighlightText value={field.key} query={query} />
              </dt>
              <dd>
                <HighlightText value={field.value} query={query} />
              </dd>
            </div>
          ))}
        </dl>
      ) : null}
    </div>
  );
}

function safeParseTracePayload(payload: string): Record<string, unknown> | null {
  try {
    return JSON.parse(payload) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function HighlightText({ value, query }: { value: string; query: string }) {
  const needle = query.trim();
  if (!needle) return <>{value}</>;
  const lowerValue = value.toLowerCase();
  const lowerNeedle = needle.toLowerCase();
  const parts: ReactNode[] = [];
  let cursor = 0;
  let matchIndex = lowerValue.indexOf(lowerNeedle);
  while (matchIndex >= 0) {
    if (matchIndex > cursor) parts.push(value.slice(cursor, matchIndex));
    parts.push(
      <mark key={`${matchIndex}-${cursor}`}>
        {value.slice(matchIndex, matchIndex + needle.length)}
      </mark>,
    );
    cursor = matchIndex + needle.length;
    matchIndex = lowerValue.indexOf(lowerNeedle, cursor);
  }
  if (cursor < value.length) parts.push(value.slice(cursor));
  return <>{parts}</>;
}

function TraceFilterTabs<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  title?: string;
  value: T;
  options: Array<[T, string]>;
  onChange: (value: T) => void;
}) {
  return (
    <div className="trace-filter-group">
      <span className="trace-filter-label">{label}</span>
      <SegmentedControl
        label={label}
        value={value}
        options={options}
        onChange={onChange}
        variant="quiet"
      />
      <TraceSelectControl
        label={label}
        value={value}
        options={options}
        onChange={onChange}
      />
    </div>
  );
}

function TraceSelectControl<T extends string>({
  label,
  value,
  options,
  onChange,
  className,
}: {
  label: string;
  title?: string;
  value: T;
  options: Array<[T, string]>;
  onChange: (value: T) => void;
  className?: string;
}) {
  const labelId = useId();
  return (
    <div className={`trace-select-control${className ? ` ${className}` : ""}`}>
      <span id={labelId}>{label}</span>
      <Select
        value={value}
        onValueChange={(nextValue) => onChange(nextValue as T)}
      >
        <SelectTrigger aria-labelledby={labelId}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {options.map(([optionValue, optionLabel]) => (
              <SelectItem key={optionValue} value={optionValue}>
                {optionLabel}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  );
}

function UsbDeveloperConsole({
  logs,
  trace,
}: {
  logs: SerialLogEntry[];
  trace: SerialTraceEntry[];
}) {
  const [expanded, setExpanded] = useState(false);
  const [wrapLines, setWrapLines] = useState(true);
  const [traceMode, setTraceMode] = useState<"raw" | "parsed" | "compare">(
    "compare",
  );
  const [levelFilter, setLevelFilter] = useState<TraceLevelFilter>("all");
  const [directionFilter, setDirectionFilter] =
    useState<TraceDirectionFilter>("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [traceScrollTop, setTraceScrollTop] = useState(0);
  const [traceViewportHeight, setTraceViewportHeight] = useState(720);
  const [measuredTraceHeights, setMeasuredTraceHeights] = useState<
    Record<string, number>
  >({});
  const [tracePinnedToBottom, setTracePinnedToBottom] = useState(true);
  const tracePanelRef = useRef<HTMLDivElement | null>(null);
  const traceAnchorRef = useRef<TraceScrollAnchor | null>(null);
  const protocolFrames = trace.filter((entry) => entry.kind === "frame").length;
  const rawLines = trace.filter((entry) => entry.kind !== "frame").length;
  const decodeStatus = useMemo(() => defmtDecodeStatus(trace), [trace]);
  const traceModeOptions: Array<["raw" | "parsed" | "compare", string]> = [
    ["raw", "Raw"],
    ["parsed", "Parsed"],
    ["compare", "Compare"],
  ];
  const normalizedQuery = searchQuery.trim().toLowerCase();
  const filteredTrace = useMemo(
    () =>
      trace.filter((entry) => {
        const level = traceEntryLevel(entry);
        const matchesLevel =
          levelFilter === "all" ||
          traceLevelRank[level] <= traceLevelRank[levelFilter];
        const matchesDirection =
          directionFilter === "all" || entry.direction === directionFilter;
        const matchesSearch =
          !normalizedQuery ||
          traceSearchText(entry, level).includes(normalizedQuery);
        return matchesLevel && matchesDirection && matchesSearch;
      }),
    [directionFilter, levelFilter, normalizedQuery, trace],
  );
  const estimatedTraceHeight =
    traceMode === "compare" ? (wrapLines ? 128 : 112) : wrapLines ? 72 : 64;
  const traceHeightKey = (entry: SerialTraceEntry) =>
    `${traceMode}:${wrapLines ? "wrap" : "nowrap"}:${entry.id}`;
  const traceLayout = useMemo(() => {
    const offsets: number[] = [];
    let totalHeight = 0;
    for (const entry of filteredTrace) {
      offsets.push(totalHeight);
      totalHeight +=
        measuredTraceHeights[traceHeightKey(entry)] ?? estimatedTraceHeight;
    }
    return { offsets, totalHeight };
  }, [
    estimatedTraceHeight,
    filteredTrace,
    measuredTraceHeights,
    traceMode,
    wrapLines,
  ]);
  const overscanPx = estimatedTraceHeight * 8;
  const virtualTop = Math.max(0, traceScrollTop - overscanPx);
  const virtualBottom = traceScrollTop + traceViewportHeight + overscanPx;
  let virtualStart = 0;
  while (
    virtualStart < filteredTrace.length &&
    traceLayout.offsets[virtualStart] +
      (measuredTraceHeights[traceHeightKey(filteredTrace[virtualStart])] ??
        estimatedTraceHeight) <
      virtualTop
  ) {
    virtualStart += 1;
  }
  let virtualEnd = virtualStart;
  while (
    virtualEnd < filteredTrace.length &&
    traceLayout.offsets[virtualEnd] < virtualBottom
  ) {
    virtualEnd += 1;
  }
  const virtualTrace = filteredTrace.slice(virtualStart, virtualEnd);
  function captureTraceAnchor(scrollTop: number) {
    traceAnchorRef.current = captureTraceScrollAnchor(
      filteredTrace,
      traceLayout.offsets,
      scrollTop,
    );
  }

  function scrollTraceToBottom() {
    const panel = tracePanelRef.current;
    const maxScrollTop = panel
      ? Math.max(0, panel.scrollHeight - panel.clientHeight)
      : Math.max(0, traceLayout.totalHeight - traceViewportHeight);
    traceAnchorRef.current = null;
    setTracePinnedToBottom(true);
    setTraceScrollTop(maxScrollTop);
    if (panel) panel.scrollTop = maxScrollTop;
  }

  useLayoutEffect(() => {
    setTracePinnedToBottom(true);
    traceAnchorRef.current = null;
  }, [directionFilter, levelFilter, normalizedQuery, traceMode, wrapLines]);

  useLayoutEffect(() => {
    const panel = tracePanelRef.current;
    if (!panel) return;
    const maxScrollTop = Math.max(
      0,
      traceLayout.totalHeight - traceViewportHeight,
    );
    const nextScrollTop = resolveAnchoredTraceScrollTop({
      anchor: traceAnchorRef.current,
      entries: filteredTrace,
      offsets: traceLayout.offsets,
      currentScrollTop: panel.scrollTop,
      maxScrollTop,
      pinnedToBottom: tracePinnedToBottom,
    });
    if (panel.scrollTop !== nextScrollTop) panel.scrollTop = nextScrollTop;
    if (traceScrollTop !== nextScrollTop) setTraceScrollTop(nextScrollTop);
    if (tracePinnedToBottom) {
      traceAnchorRef.current = null;
    } else {
      captureTraceAnchor(nextScrollTop);
    }
  }, [
    filteredTrace,
    traceLayout,
    tracePinnedToBottom,
    traceScrollTop,
    traceViewportHeight,
  ]);

  useEffect(() => {
    const panel = tracePanelRef.current;
    if (!panel) return;
    const updateViewportHeight = () =>
      setTraceViewportHeight(panel.clientHeight || 720);
    updateViewportHeight();
    const observer = new ResizeObserver(updateViewportHeight);
    observer.observe(panel);
    return () => observer.disconnect();
  }, [expanded]);

  function measureTraceItem(
    entry: SerialTraceEntry,
    node: HTMLDivElement | null,
  ) {
    if (!node) return;
    const key = traceHeightKey(entry);
    const measuredHeight = Math.ceil(node.getBoundingClientRect().height);
    if (!measuredHeight) return;
    if (measuredTraceHeights[key] === measuredHeight) return;
    setMeasuredTraceHeights((current) =>
      current[key] === measuredHeight
        ? current
        : { ...current, [key]: measuredHeight },
    );
  }

  const renderRawRow = (
    entry: SerialTraceEntry,
    key: string,
    className = `trace-row kind-${entry.kind}`,
  ) => {
    const hasDistinctPayload =
      entry.kind === "frame" || entry.payload !== entry.summary;
    return (
      <div className={className} key={key}>
        <span>{new Date(entry.timestamp).toLocaleTimeString()}</span>
        <strong>{entry.direction}</strong>
        <code>raw</code>
        <em>{entry.requestId ?? entry.target ?? "--"}</em>
        <p className={hasDistinctPayload ? "" : "trace-message-inline"}>
          <HighlightText
            value={
              hasDistinctPayload && entry.kind === "frame"
                ? "raw JSONL frame"
                : entry.summary
            }
            query={searchQuery}
          />
        </p>
        {hasDistinctPayload ? (
          <pre>
            <TraceMessage entry={entry} query={searchQuery} mode="raw" />
          </pre>
        ) : null}
      </div>
    );
  };
  const renderParsedRow = (
    entry: SerialTraceEntry,
    key: string,
    className = `trace-row kind-${entry.kind}`,
  ) => (
    <div className={className} key={key}>
      <span>{new Date(entry.timestamp).toLocaleTimeString()}</span>
      <strong>{entry.direction}</strong>
      <code>{entry.frameType ?? entry.kind}</code>
      <em>{entry.requestId ?? entry.target ?? "--"}</em>
      <p>
        <HighlightText value={traceSummaryLabel(entry)} query={searchQuery} />
      </p>
      <div className="trace-row-body">
        {entry.kind === "frame" ? (
          <HighlightText
            value={`${entry.frameType ?? "frame"} ${entry.requestId ?? entry.target ?? ""}`.trim()}
            query={searchQuery}
          />
        ) : (
          <TraceMessage entry={entry} query={searchQuery} mode="summary" />
        )}
      </div>
    </div>
  );
  const renderTraceEntry = (entry: SerialTraceEntry) => {
    if (traceMode === "raw") return renderRawRow(entry, entry.id);
    if (traceMode === "parsed") return renderParsedRow(entry, entry.id);
    return (
      <div className={`trace-compare-group kind-${entry.kind}`} key={entry.id}>
        {renderParsedRow(entry, `${entry.id}-parsed`)}
        {renderRawRow(
          entry,
          `${entry.id}-raw`,
          "trace-row trace-row-original kind-raw",
        )}
      </div>
    );
  };

  return (
    <section
      className={`info-panel developer-console ${expanded ? "is-expanded" : ""} ${wrapLines ? "wrap-lines" : "no-wrap-lines"}`}
      data-evidence-target="usb-developer-console"
    >
      <header className="developer-console-header">
        <div className="developer-console-title">
          <Terminal size={18} />
          <h2>USB Console</h2>
          <TraceHelpBubble status={decodeStatus} />
        </div>
        <div className="developer-console-actions">
          <TraceSelectControl
            label="View"
            value={traceMode}
            options={traceModeOptions}
            onChange={setTraceMode}
            className="trace-mode-select"
          />
          <SegmentedControl
            label="Trace view mode"
            value={traceMode}
            options={traceModeOptions}
            onChange={setTraceMode}
            variant="compact"
            className="trace-mode-tabs"
            getOptionTitle={(mode) =>
              ({
                raw: "Show the decoded defmt line when available, otherwise show the captured CDC payload.",
                parsed:
                  "Show human-readable defmt fields and hide the original payload.",
                compare:
                  "Show the parsed view together with the original payload for debugging.",
              })[mode]
            }
          />
          <TraceFilterTabs<TraceLevelFilter>
            label="Level"
            value={levelFilter}
            options={[
              ["all", "All"],
              ["error", "Error"],
              ["warn", "Warn+"],
              ["info", "Info+"],
              ["debug", "Debug+"],
              ["trace", "Trace+"],
            ]}
            onChange={setLevelFilter}
          />
          <TraceFilterTabs<TraceDirectionFilter>
            label="Direction"
            value={directionFilter}
            options={[
              ["all", "All"],
              ["rx", "RX"],
              ["tx", "TX"],
            ]}
            onChange={setDirectionFilter}
          />
          <label className="trace-search">
            <Search size={14} />
            <input
              type="search"
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              placeholder="Search logs"
              aria-label="Search USB console logs"
            />
          </label>
          <span className="trace-filter-count">
            {filteredTrace.length} shown
          </span>
          <div className="developer-console-ops">
            <button
              className={`trace-live-button ${tracePinnedToBottom ? "is-following" : ""}`}
              type="button"
              onClick={scrollTraceToBottom}
              aria-pressed={tracePinnedToBottom}
              title={
                tracePinnedToBottom
                  ? "The console is following new records."
                  : "Jump back to the newest record and follow live updates."
              }
            >
              {tracePinnedToBottom ? "Following latest" : "Resume live"}
            </button>
            <label className="switch-control">
              <input
                type="checkbox"
                checked={wrapLines}
                onChange={(event) => setWrapLines(event.target.checked)}
              />
              <span>Wrap lines</span>
            </label>
            <button
              className="icon-button"
              type="button"
              onClick={() => setExpanded((current) => !current)}
              aria-label={
                expanded ? "Exit fullscreen console" : "Open fullscreen console"
              }
              title={expanded ? "Exit fullscreen" : "Fullscreen"}
            >
              {expanded ? <Minimize2 size={16} /> : <Maximize2 size={16} />}
            </button>
          </div>
        </div>
      </header>
      <div className="developer-console-metrics">
        <MetricLine
          label="CDC records"
          value={String(trace.length)}
          title="All USB CDC records captured in the current in-memory trace window."
        />
        <MetricLine
          label="Protocol frames"
          value={String(protocolFrames)}
          title="Structured command or response frames recognized by the app protocol parser."
        />
        <MetricLine
          label="Structured logs"
          value={String(logs.length)}
          title="Application log entries that were parsed into structured state."
        />
        <MetricLine
          label="Raw / ignored"
          value={String(rawLines)}
          title="Records that are not app protocol frames. This can include decoded defmt lines, plain text, or ignored binary payloads."
        />
      </div>
      <div
        className="trace-panel is-virtualized"
        ref={tracePanelRef}
        role="log"
        aria-label="USB CDC trace records"
        aria-live="off"
        onScroll={(event) => {
          const panel = event.currentTarget;
          const maxScrollTop = Math.max(
            0,
            panel.scrollHeight - panel.clientHeight,
          );
          const pinned = maxScrollTop - panel.scrollTop < 24;
          setTracePinnedToBottom(pinned);
          setTraceScrollTop(panel.scrollTop);
          if (pinned) {
            traceAnchorRef.current = null;
          } else {
            captureTraceAnchor(panel.scrollTop);
          }
        }}
      >
        {filteredTrace.length > 0 ? (
          <div
            className="trace-virtual-spacer"
            style={{ height: traceLayout.totalHeight }}
          >
            {virtualTrace.map((entry, index) => (
              <div
                className="trace-virtual-item"
                key={entry.id}
                ref={(node) => measureTraceItem(entry, node)}
                style={{
                  transform: `translateY(${traceLayout.offsets[virtualStart + index]}px)`,
                }}
              >
                {renderTraceEntry(entry)}
              </div>
            ))}
          </div>
        ) : (
          <p className="panel-note">
            No CDC records match the current filters.
          </p>
        )}
      </div>
    </section>
  );
}

function StatusPair({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </>
  );
}

function BatteryLevelIcon({
  status,
}: {
  status: UpsStatus | null | undefined;
}) {
  const soc = status?.battery.soc_pct;
  const Icon =
    soc === null || soc === undefined
      ? BatteryWarning
      : soc < 20
        ? BatteryWarning
        : soc < 45
          ? BatteryLow
          : soc < 75
            ? BatteryMedium
            : BatteryFull;
  return <Icon size={18} aria-hidden="true" />;
}

function PowerMetricIcon({ record }: { record: DeviceRecord }) {
  const source = powerSourceLabel(record);
  if (source === "Battery")
    return <BatteryBackupIcon size={18} aria-hidden="true" />;
  const Icon =
    source === "Offline" || source === "No mains" ? AlertTriangle : PlugZap;
  return <Icon size={18} aria-hidden="true" />;
}

function BatteryBackupIcon({
  size = 18,
  ...props
}: SVGProps<SVGSVGElement> & { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" {...props}>
      <rect x="3" y="7" width="16" height="10" rx="2.4" fill="currentColor" />
      <rect x="20" y="10" width="2" height="4" rx="0.8" fill="currentColor" />
    </svg>
  );
}

function SeverityBadge({ severity }: { severity: Severity }) {
  return (
    <span className={`severity-badge severity-${severity}`}>{severity}</span>
  );
}

function pressureSeverityForState(
  state: string | null | undefined,
): Severity {
  switch (state) {
    case "limited":
    case "cooldown":
      return "critical";
    case "watch":
      return "warning";
    case "headroom":
      return "ok";
    default:
      return "info";
  }
}

function powerReasonLabel(reason: string | null | undefined): string {
  switch (reason) {
    case "user_stop":
      return "User stop";
    case "timer_expired":
      return "Timer expired";
    case "pack_reached":
      return "Pack target reached";
    case "rsoc_reached":
      return "RSOC target reached";
    case "full_reached":
      return "Full target reached";
    case "safety_blocked":
      return "Safety blocked";
    case "manual_charge_blocked_safety":
      return "Safety blocked";
    case "manual_charge_blocked_no_input":
      return "No usable input";
    case "manual_charge_blocked_temp":
      return "Temperature blocked";
    case "manual_charge_blocked_no_bms":
      return "Battery charge authorization unavailable";
    case "manual_charge_blocked_output_overload":
      return "Output overload";
    case "manual_charge_path_unavailable":
      return "Requested path unavailable";
    case "manual_charge_path_not_qualified":
      return "USB-C not charge-qualified";
    case "loop_confirmation_required":
      return "Loop confirmation required";
    case "auto_dcin_fallback":
      return "Auto selected DCIN";
    case "auto_high_power_usb_pd":
      return "Auto selected high-power USB-C PD";
    case "auto_usbc_fallback":
      return "Auto selected USB-C";
    case "explicit_dcin":
      return "Manual DCIN";
    case "explicit_usbc":
      return "Manual USB-C";
    case "tps_output_current":
      return "TPS output current";
    case "pressure_tps_output_current":
      return "TPS output current limit";
    case "cooldown_retry_wait":
      return "Cooldown retry wait";
    case "recovery_hold":
      return "Recovery hold";
    case "startup_ramp":
      return "Startup ramp";
    case "pressure_vindpm":
      return "VINDPM";
    case "pressure_iindpm":
      return "IINDPM";
    case "pressure_poorsrc":
      return "Poor source";
    case "xchg_blocked":
      return "BMS charge path blocked";
    case "chg_fet_off":
      return "Battery charge FET off";
    case "xdsg_blocked":
      return "BMS discharge path blocked";
    case "dsg_fet_off":
      return "Battery discharge FET off";
    case "cell_undervoltage":
      return "Cell undervoltage";
    case "remaining_capacity_alarm":
      return "Remaining-capacity alarm";
    case "permanent_failure":
      return "Permanent failure";
    case "sleep_mode":
      return "Sleep mode";
    case "pack_output_path_open":
      return "Pack output path open";
    case "physical_vbat_absent":
      return "Pack VBAT absent at charger";
    case "op_status_unavailable":
      return "BMS status unavailable";
    case "sbs_error_code":
      return "BMS SBS error";
    case "no_battery":
      return "Pack not detected";
    case "vin_drop_watch":
      return "VIN drop watch";
    case "vin_drop":
      return "VIN drop";
    case "none":
    case null:
    case undefined:
      return "none";
    default:
      return reason;
  }
}

function batteryIssueDetailSummary(reason: string): string {
  const label = powerReasonLabel(reason);
  return label === reason ? reason : `${label} (${reason})`;
}

function manualChargeBmsBlockCause(
  status: UpsStatus | null | undefined,
): string | null {
  if (!status) return null;
  if (status.battery.no_battery) {
    return "the battery pack is not detected";
  }
  switch (status.battery.issue_detail) {
    case "xchg_blocked":
      return "the battery management system is blocking the charge path (XCHG)";
    case "chg_fet_off":
      return "the battery charge FET is off";
    case "xdsg_blocked":
      return "the battery discharge path is blocked by BMS";
    case "dsg_fet_off":
      return "the battery discharge FET is off";
    case "cell_undervoltage":
      return "the battery is below the cell-undervoltage recovery gate";
    case "remaining_capacity_alarm":
      return "the battery raised a remaining-capacity alarm";
    case "permanent_failure":
      return "the battery reports a permanent failure";
    case "sleep_mode":
      return "the battery is in sleep mode";
    case "pack_output_path_open":
      return "the pack output path is open";
    case "physical_vbat_absent":
      return "the charger does not see pack voltage on VBAT";
    case "op_status_unavailable":
      return "the BMS operation-status word is unavailable";
    case "sbs_error_code":
      return "the battery reported an SBS error";
    case "no_battery":
      return "the battery pack is not detected";
    default:
      break;
  }
  if (status.battery.charge_fet_on === false) {
    return "the battery charge FET is off";
  }
  if (status.charger.vbat_present === false) {
    return "the charger does not detect battery voltage";
  }
  if (status.battery.discharge_ready === false) {
    return "battery authorization telemetry is not ready";
  }
  return null;
}

function powerThresholdSummary(
  actualMa: number | null | undefined,
  thresholdMa: number | null | undefined,
): string {
  if (typeof actualMa === "number" && typeof thresholdMa === "number") {
    return `${actualMa} mA / ${thresholdMa} mA`;
  }
  if (typeof actualMa === "number") return `${actualMa} mA`;
  if (typeof thresholdMa === "number") return `-- / ${thresholdMa} mA`;
  return "--";
}

function powerStopSummary(
  reason: string | null | undefined,
  actualMa: number | null | undefined,
  thresholdMa: number | null | undefined,
): string {
  if (reason === "tps_output_current" && typeof actualMa === "number" && typeof thresholdMa === "number") {
    return `Stopped: TPS output current ${actualMa} mA > ${thresholdMa} mA`;
  }
  if (
    reason === "tps_output_current" ||
    reason === "pressure_tps_output_current" ||
    reason === "cooldown"
  ) {
    const summary = powerThresholdSummary(actualMa, thresholdMa);
    return summary === "--"
      ? "Stopped: TPS output current over limit"
      : `Stopped: TPS output current ${summary.replace(" / ", " > ")}`;
  }
  return powerReasonLabel(reason);
}

function asNullableNumber(value: unknown): number | null {
  return typeof value === "number" ? value : null;
}

function powerEventSummary(payload: Record<string, unknown> | null): string {
  const pressureState = String(payload?.pressure_state ?? "unknown");
  const limitReason = powerReasonLabel(
    typeof payload?.limit_reason === "string" ? payload.limit_reason : null,
  );
  const pressureReason = powerReasonLabel(
    typeof payload?.pressure_reason === "string" ? payload.pressure_reason : null,
  );
  const actualMa = asNullableNumber(payload?.tps_total_iout_ma);
  const thresholdMa = asNullableNumber(payload?.tps_limit_threshold_ma);
  if (
    typeof payload?.pressure_reason === "string" &&
    payload.pressure_reason === "tps_output_current" &&
    typeof actualMa === "number" &&
    typeof thresholdMa === "number"
  ) {
    return `Stopped: TPS output current ${actualMa} mA > ${thresholdMa} mA (${pressureState}, ${limitReason})`;
  }
  return `pressure ${pressureState} / reason ${pressureReason} / limit ${limitReason}`;
}

function DeviceRoutePlaceholder({
  title,
  state,
}: {
  title: string;
  state: "loading" | "missing";
}) {
  const loading = state === "loading";
  return (
    <section className="device-route-placeholder">
      <header className="page-header">
        <div>
          <div className="eyebrow">Device</div>
          <h1>{title}</h1>
        </div>
      </header>
      <div className="device-data-context tone-warning" role="status">
        <span className="eyebrow">Device state</span>
        <strong>{loading ? "Connecting" : "Offline"}</strong>
        <span>
          {loading
            ? "Waiting for the device connection to resolve."
            : "The device is not currently available to this page."}
        </span>
      </div>
      <div className="empty-state">
        {loading ? <Loader2 size={28} className="spin-icon" /> : <Server size={28} />}
        <h2>{loading ? "Loading device" : "Device not found"}</h2>
        <p>
          {loading
            ? "Loading the selected device before this page renders."
            : "The selected device is no longer available in the fleet or local registry."}
        </p>
        {!loading ? (
          <button className="primary-button" onClick={() => navigate("/")}>
            Back to fleet
          </button>
        ) : null}
      </div>
    </section>
  );
}

export function resolveUpsHardwareCapability(
  record: Pick<DeviceRecord, "identity" | "settings">,
): UpsHardwareCapability {
  const identityCapability = record.identity?.hardware_capabilities;
  if (identityCapability && typeof identityCapability.rated_vout_mv === "number") {
    return {
      outputProfile:
        normalizeOutputProfile(identityCapability.output_profile) ??
        inferOutputProfileFromRatedVout(identityCapability.rated_vout_mv),
      ratedVoutMv: identityCapability.rated_vout_mv,
      source: "identity",
    };
  }
  const firmwareOutputProfile = firmwareOutputProfileFallback(
    record.identity?.firmware.features,
  );
  if (firmwareOutputProfile) {
    return {
      outputProfile: firmwareOutputProfile,
      ratedVoutMv: ratedVoutFromOutputProfile(firmwareOutputProfile),
      source: "firmware",
    };
  }
  const settingsRatedVout = record.settings?.advanced_power_capabilities?.rated_vout_mv;
  if (typeof settingsRatedVout === "number") {
    return {
      outputProfile: inferOutputProfileFromRatedVout(settingsRatedVout),
      ratedVoutMv: settingsRatedVout,
      source: "settings",
    };
  }
  return {
    outputProfile: null,
    ratedVoutMv: null,
    source: "unknown",
  };
}

function normalizeOutputProfile(
  outputProfile: string | null | undefined,
): string | null {
  const value = outputProfile?.trim().toLowerCase();
  return value ? value : null;
}

function inferOutputProfileFromRatedVout(
  ratedVoutMv: number | null | undefined,
): string | null {
  if (
    typeof ratedVoutMv !== "number" ||
    !Number.isFinite(ratedVoutMv) ||
    ratedVoutMv <= 0
  ) {
    return null;
  }
  const volts = ratedVoutMv / 1000;
  const normalizedVolts = Number.isInteger(volts)
    ? String(volts)
    : volts.toFixed(1).replace(/\.0$/, "");
  return `${normalizedVolts}v`;
}

function firmwareOutputProfileFallback(
  features: readonly string[] | null | undefined,
): string | null {
  if (!Array.isArray(features)) return null;
  if (features.includes("main-vout-19v")) return "19v";
  if (features.includes("main-vout-12v")) return "12v";
  return null;
}

function ratedVoutFromOutputProfile(
  outputProfile: string | null | undefined,
): number | null {
  const normalized = normalizeOutputProfile(outputProfile);
  if (normalized === "19v") return 19_000;
  if (normalized === "12v") return 12_000;
  return null;
}

function hardwareOutputProfileLabel(
  outputProfile: string | null | undefined,
): string {
  const normalized = normalizeOutputProfile(outputProfile);
  if (!normalized) return "--";
  if (normalized.endsWith("v")) {
    return `${normalized.slice(0, -1).toUpperCase()}V`;
  }
  return normalized.toUpperCase();
}

function ratedVoutMillivoltLabel(value: number | null | undefined): string {
  return typeof value === "number" ? `${value} mV` : "--";
}

function hardwareCapabilityHeadline(
  capability: UpsHardwareCapability,
): string {
  if (capability.outputProfile)
    return `${hardwareOutputProfileLabel(capability.outputProfile)} profile`;
  if (capability.ratedVoutMv !== null)
    return `${formatVoltage(capability.ratedVoutMv)} rated`;
  return "Capability pending";
}

function hardwareCapabilityMetricDetail(
  capability: UpsHardwareCapability,
): string {
  const segments = [
    `output_profile=${capability.outputProfile ?? "--"}`,
    `rated_vout_mv=${capability.ratedVoutMv ?? "--"}`,
  ];
  if (capability.source === "settings") segments.push("settings fallback");
  if (capability.source === "firmware") segments.push("firmware fallback");
  return segments.join(" · ");
}

function hardwareCapabilityDetail(
  capability: UpsHardwareCapability,
): string {
  const suffix =
    capability.source === "settings"
      ? "This is a settings fallback until hardware identity reports the capability fields."
      : capability.source === "firmware"
        ? "This is inferred from the active firmware output profile until hardware identity reports the capability fields."
      : capability.source === "unknown"
        ? "Hardware capability fields are still pending."
        : "Advanced-power offsets below stay relative to this rated output.";
  return `${hardwareCapabilityMetricDetail(capability)}. ${suffix}`;
}

function hardwareCapabilityOutputProfileLabel(
  capability: UpsHardwareCapability,
): string {
  if (!capability.outputProfile) return "--";
  return capability.source === "settings" || capability.source === "firmware"
    ? `${capability.outputProfile} (inferred)`
    : capability.outputProfile;
}

function hardwareCapabilitySourceLabel(
  source: UpsHardwareCapability["source"],
): string {
  if (source === "identity") return "Hardware identity";
  if (source === "firmware") return "Firmware output profile fallback";
  if (source === "settings") return "Advanced-power settings fallback";
  return "Not reported";
}

function hardwareCapabilitySummary(record: DeviceRecord): string {
  const capability = resolveUpsHardwareCapability(record);
  if (capability.outputProfile && capability.ratedVoutMv !== null) {
    return `${hardwareOutputProfileLabel(capability.outputProfile)} / ${ratedVoutMillivoltLabel(capability.ratedVoutMv)}`;
  }
  if (capability.ratedVoutMv !== null) {
    return ratedVoutMillivoltLabel(capability.ratedVoutMv);
  }
  return "Pending";
}

function powerSourceLabel(record: DeviceRecord): string {
  if (record.connectionState === "offline") return "Offline";
  const status = record.status;
  if (!status) return "--";
  if (status.mode === "backup") return "Battery";
  if (status.input.mains_present === true) return "Mains";
  if (status.input.mains_present === false) return "No mains";
  return "--";
}

function loadSummary(record: DeviceRecord): string {
  if (record.connectionState === "offline") return "Unknown";
  const status = record.status;
  if (!status) return "--";
  if (status.output.active === "none") return "Not powered";
  if (status.output.active === "both") return "Powered";
  if (status.output.active === "out_a" || status.output.active === "out_b")
    return "Partially powered";
  return "--";
}

function batterySummary(record: DeviceRecord): string {
  if (record.connectionState === "offline") return "Unknown";
  const status = record.status;
  const battery = status?.battery;
  if (!battery) return "--";
  if (battery.no_battery) return "Not detected";
  if (battery.state === "fault") return "Needs check";
  if (battery.soc_pct !== null && battery.soc_pct < 20) return "Low";
  if (battery.discharge_ready === false) return "Not ready";
  return "Ready";
}

function attentionSummary(record: DeviceRecord): string {
  const status = record.status;
  const severity = deviceSeverity(record);
  if (severity === "offline") return "Reconnect device";
  if (!status) return record.error?.message ?? "--";
  if (status.output.gate_reason && status.output.gate_reason !== "none")
    return "Protection active";
  if (
    status.thermal?.tmp_a_state === "hot" ||
    status.thermal?.tmp_b_state === "hot"
  )
    return "High temperature";
  if (status.battery.issue_detail) return "Battery attention";
  if (severity === "critical") return "Immediate attention";
  if (severity === "warning") return "Check soon";
  if (status.mode === "backup") return "On battery";
  return "Normal";
}

function connectionSummary(record: DeviceRecord): string {
  if (record.connectionState === "online") return "Online";
  if (record.connectionState === "connecting") return "Connecting";
  if (record.connectionState === "offline") return "Offline";
  if (
    record.network?.state === "connected" ||
    record.status?.network.state === "connected"
  )
    return "Online";
  return "Check connection";
}

function connectionEndpointLabel(record: DeviceRecord): string {
  const activeTransport = activeRecordTransport(record);
  if (activeTransport === "serial") {
    return `${record.target.serialProtocol ?? record.serial?.protocol ?? "USB CDC"} · ${record.serial?.connected ? "connected" : "disconnected"}`;
  }
  if (activeTransport === "devd") {
    return `${rememberedDevdBaseUrl(record) ?? record.target.baseUrl} · devd USB ${record.serial?.connected ? "connected" : "disconnected"}`;
  }
  if (activeTransport === "http") {
    return rememberedHttpBaseUrl(record) ?? record.target.baseUrl;
  }
  const rememberedChannels = availableRecordChannels(record);
  if (rememberedChannels.length > 0) {
    return `Remembered ${rememberedChannels.map((transport) => channelBadgeLabel(transport)).join(" / ")}`;
  }
  return record.target.baseUrl;
}

function companionChannelSummary(record: DeviceRecord): string | null {
  const httpChannel = record.target.rememberedChannels?.http;
  const devdChannel = record.target.rememberedChannels?.devd;
  if (!httpChannel && !devdChannel) return null;
  const segments: string[] = [];
  segments.push(
    `Preferred ${channelBadgeLabel(preferredRecordTransport(record))}`,
  );
  if (httpChannel?.baseUrl) {
    segments.push(`Web direct ${httpChannel.baseUrl}`);
  }
  if (httpChannel?.fallbackBaseUrl) {
    segments.push(`WiFi fallback ${httpChannel.fallbackBaseUrl}`);
  }
  if (httpChannel?.mdnsHost) {
    segments.push(`devd mDNS ${httpChannel.mdnsHost}`);
  }
  return segments.join(" · ");
}

function splitErrorMessage(message: string): [string | null, string] {
  const separator = message.indexOf(":");
  if (separator === -1) return [null, message];
  return [
    message.slice(0, separator).trim(),
    message.slice(separator + 1).trim(),
  ];
}

function channelLabel(
  channel: UpsStatus["output"]["out_a"] | undefined,
): string {
  if (!channel) return "--";
  return `${channel.enabled ? "on" : "off"} / ${channel.state}`;
}

function boolLabel(
  value: boolean | null | undefined,
  trueLabel: string,
  falseLabel: string,
): string {
  if (value === true) return trueLabel;
  if (value === false) return falseLabel;
  return "--";
}

function maxTemp(status: UpsStatus | null | undefined): number | null {
  const values = [status?.thermal.tmp_a_c, status?.thermal.tmp_b_c].filter(
    (value): value is number => typeof value === "number",
  );
  if (values.length === 0) return null;
  return Math.max(...values);
}
