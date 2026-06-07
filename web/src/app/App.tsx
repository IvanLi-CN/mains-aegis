import {
  Activity,
  AlertTriangle,
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
  Globe2,
  KeyRound,
  LayoutGrid,
  Loader2,
  Maximize2,
  Menu,
  Minimize2,
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
  type ReactNode,
  type SVGProps,
} from "react";
import type { LucideIcon } from "lucide-react";
import {
  bindDevdDevice,
  isHostedHttpServiceApp,
  listDevdDevices,
  normalizeBaseUrl,
  subscribeDevdDeviceEvents,
  toErrorEnvelope,
} from "../api/client";
import type {
  DeviceRecord,
  DeviceSettings,
  DevdDevice,
  SerialLogEntry,
  SerialTraceEntry,
  UpsStatus,
} from "../api/types";
import { SegmentedControl } from "../components/ui/segmented-control";
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
  type DeviceChannelTransport,
  type WifiProvisioningProgress,
} from "../device-registry/context";
import { isDemoSeed } from "../fixtures/mockDevices";
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

type SharedDevdDiscovery = {
  devdTarget: string | null;
  devdDevices: DevdDevice[];
  status: FleetDiscoveryStatus;
  isRefreshing: boolean;
  lastUpdated: string | null;
  refresh: () => Promise<void>;
};

const deviceSections = [
  { id: "overview", label: "Overview", icon: Gauge },
  { id: "power", label: "Power", icon: PlugZap },
  { id: "battery", label: "Battery", icon: BatteryCharging },
  { id: "thermal", label: "Thermal", icon: Thermometer },
  { id: "device", label: "Device", icon: Cpu },
  { id: "firmware", label: "Firmware", icon: FileDown },
  { id: "settings", label: "Settings", icon: Settings },
  { id: "api", label: "API", icon: Cable },
] as const;

const appBasePath = normalizeBasePath(import.meta.env.BASE_URL);
const envDevdTarget =
  (
    import.meta.env.VITE_DEFAULT_DEVD_URL ??
    import.meta.env.VITE_DEVD_API_BASE ??
    "same-origin"
  ).trim() || "same-origin";
const docsHref = `${appBasePath}docs/`;
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
  const demoMode = isDemoSeed(
    new URLSearchParams(window.location.search).get("seed"),
  );
  const hostedHttpServiceApp =
    forceHostedHttpServiceApp ?? isHostedHttpServiceApp();
  const devdTarget = resolveDevdTarget(
    initialDevdTarget,
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
  const selected = route.deviceId
    ? (registry.records.find(
        (record) => record.target.deviceId === route.deviceId,
      ) ?? null)
    : null;
  const [navOpen, setNavOpen] = useState(false);

  useEffect(() => {
    setNavOpen(false);
  }, [route.path]);

  return (
    <div className="app-shell">
      <aside className={`sidebar ${navOpen ? "is-open" : ""}`}>
        <div className="mobile-nav-bar">
          <button
            className="icon-button"
            type="button"
            aria-label={navOpen ? "Close navigation" : "Open navigation"}
            onClick={() => setNavOpen((open) => !open)}
          >
            {navOpen ? <X size={18} /> : <Menu size={18} />}
          </button>
          <div className="mobile-nav-title">
            <strong>Mains Aegis</strong>
            <span>
              {route.section === "connect"
                ? "Connect"
                : (selected?.target.alias ?? "Fleet")}
            </span>
          </div>
        </div>
        <button
          className="mobile-nav-backdrop"
          type="button"
          aria-label="Close navigation"
          onClick={() => setNavOpen(false)}
        />
        <div className="sidebar-panel">
          <div className="brand">
            <span className="brand-mark">MA</span>
            <div>
              <strong>Mains Aegis</strong>
              <span>UPS fleet console</span>
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
              label="Connect"
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
        <TopBar records={fleetRecords} selected={selected} />
        {renderRoute(
          route,
          fleetEntries,
          selected,
          initialDevdTarget,
          hostedHttpServiceApp,
          devdDiscovery,
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
) {
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
  if (!selected) return <MissingDevice />;

  switch (route.section) {
    case "power":
      return <PowerPage record={selected} />;
    case "battery":
      return <BatteryPage record={selected} />;
    case "thermal":
      return <ThermalPage record={selected} />;
    case "device":
      return <DeviceInfoPage record={selected} />;
    case "firmware":
      return <FirmwarePageView record={selected} />;
    case "settings":
      return <SettingsPage record={selected} />;
    case "api":
      return <ApiDebugPage record={selected} />;
    default:
      return <DeviceOverviewPage record={selected} />;
  }
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

function resolveDevdTarget(
  initialDevdTarget: string | undefined,
  hostedHttpServiceApp: boolean,
  demoMode: boolean,
): string | null {
  if (demoMode && !hostedHttpServiceApp && !initialDevdTarget) return null;
  const candidate = (
    initialDevdTarget ??
    (hostedHttpServiceApp ? "same-origin" : envDevdTarget)
  ).trim();
  if (!candidate) return null;
  return candidate;
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
      applyDiscoverySnapshot(devdBaseUrl, devices.devices);
    } catch {
      setStatus(hasDiscoverySnapshot.current ? "available" : "unavailable");
      if (!hasDiscoverySnapshot.current) {
        setDevdDevices([]);
        setLastUpdated(null);
      }
    } finally {
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
    const section = (match[2] ?? "overview") as Route["section"];
    return { path, deviceId: decodeURIComponent(match[1]), section };
  }
  return { path, deviceId: null, section: "fleet" };
}

function navigate(path: string) {
  const next = new URL(withBasePath(path), window.location.origin);
  const currentSeed = new URLSearchParams(window.location.search).get("seed");
  if (!next.search && currentSeed) next.searchParams.set("seed", currentSeed);
  window.history.pushState(
    null,
    "",
    `${next.pathname}${next.search}${next.hash}`,
  );
  window.dispatchEvent(new PopStateEvent("popstate"));
}

function normalizeBasePath(base: string): string {
  if (!base || base === "/") return "/";
  const withLeading = base.startsWith("/") ? base : `/${base}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
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

function TopBar({
  records,
  selected,
}: {
  records: DeviceRecord[];
  selected: DeviceRecord | null;
}) {
  const counts = useMemo(() => {
    const severities = records.map(deviceSeverity);
    return {
      total: records.length,
      online: records.filter((record) => record.connectionState === "online")
        .length,
      critical: severities.filter((severity) => severity === "critical").length,
      warning: severities.filter((severity) => severity === "warning").length,
      offline: severities.filter((severity) => severity === "offline").length,
    };
  }, [records]);

  const title = selected ? selected.target.alias : "UPS Fleet";
  const eyebrow = selected ? selected.target.location : "Fleet";

  return (
    <header className="topbar">
      <div>
        <div className="eyebrow">{eyebrow}</div>
        <h1>{title}</h1>
      </div>
      <div className="topbar-metrics">
        <Metric label="Total" value={counts.total} />
        <Metric label="Online" value={counts.online} />
        <Metric
          label="Critical"
          value={counts.critical}
          tone={counts.critical > 0 ? "critical" : "ok"}
        />
        <Metric
          label="Warning"
          value={counts.warning}
          tone={counts.warning > 0 ? "warning" : "ok"}
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
          : "No device is saved in this browser, and mains-aegis-devd is not reporting any current device records. Open Connect to review devd records or save a device."}
      </p>
      <button
        className="primary-button"
        type="button"
        onClick={() => navigate("/connect")}
      >
        Connect devices
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
            aria-label={
              saved
                ? `Open ${record.target.alias} details`
                : `Open ${record.target.alias} in connect`
            }
            onClick={() => navigate(saved ? deviceHref(record.target.deviceId, "overview") : "/connect")}
          >
            {saved ? "Details" : "Open in Connect"}
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
  if (!device.lan_address) return device.transport === "lan" ? device : null;
  if (device.transport === "lan") return device;
  return {
    ...device,
    transport: "lan",
    port_path: null,
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
    entries.set(record.target.deviceId, {
      key: record.target.deviceId,
      record,
      saved: true,
    });
  }
  if (!devdTarget) return Array.from(entries.values());
  const devdBaseUrl = normalizeBaseUrl(devdTarget);
  for (const discovered of buildDiscoveredLogicalDevices(devdDevices, records)) {
    if (!discovered.deviceId) continue;
    const current = entries.get(discovered.deviceId);
    const mergedRecord = buildFleetEntryRecord(discovered, current?.record, devdBaseUrl);
    if (!mergedRecord) continue;
    entries.set(discovered.deviceId, {
      key: discovered.deviceId,
      record: mergedRecord,
      saved: current?.saved ?? Boolean(discovered.existingRecord),
    });
  }
  return Array.from(entries.values());
}

function buildFleetEntryRecord(
  discovered: DiscoveredLogicalDevice,
  existingRecord: DeviceRecord | undefined,
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
  const target = {
    deviceId,
    baseUrl:
      devdLanBaseUrl(httpDevice, identity) ??
      existingRecord?.target.baseUrl ??
      devdBaseUrl,
    alias:
      existingRecord?.target.alias ??
      identity?.hostname ??
      discovered.displayName,
    location: existingRecord?.target.location ?? "devd records",
    addedAt: existingRecord?.target.addedAt ?? new Date().toISOString(),
    transport:
      existingRecord?.target.transport ??
      (httpDevice ? "http" : "devd"),
    preferredTransport:
      existingRecord?.target.preferredTransport ??
      (httpDevice ? "http" : "devd"),
    rememberedChannels: {
      ...existingRecord?.target.rememberedChannels,
      ...(httpDevice
        ? {
            http: {
              baseUrl:
                devdLanBaseUrl(httpDevice, identity) ??
                existingRecord?.target.rememberedChannels?.http?.baseUrl ??
                existingRecord?.target.baseUrl ??
                "",
              seenAt: new Date().toISOString(),
              source: "devd_discovery" as const,
            },
          }
        : {}),
      ...(devdDevice
        ? {
            devd: {
              baseUrl: devdBaseUrl,
              devdDeviceId: devdDevice.id,
              seenAt: new Date().toISOString(),
              transport: devdDevice.transport === "mock" ? "mock" : "usb",
            },
          }
        : {}),
    },
  } satisfies DeviceRecord["target"];
  const connected =
    httpDevice?.connection === "connected" ||
    devdDevice?.connection === "connected";
  const connecting =
    httpDevice?.connection === "busy" || devdDevice?.connection === "busy";
  const errored =
    httpDevice?.connection === "error" || devdDevice?.connection === "error";
  return {
    target,
    identity,
    network: identity?.network ?? existingRecord?.network ?? null,
    settings: existingRecord?.settings ?? null,
    status:
      httpDevice?.status ??
      devdDevice?.status ??
      existingRecord?.status ??
      null,
    connectionState: connected
      ? "online"
      : connecting
        ? "connecting"
        : errored
          ? "error"
          : (existingRecord?.connectionState ?? "offline"),
    streamState: existingRecord?.streamState ?? "idle",
    error: existingRecord?.error ?? null,
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

function ConnectPage({
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
    addDevice,
    addDevdDevice,
    connectUsbSerialDevice,
    connectKnownDeviceChannel,
    rememberDiscoveredChannels,
    attachMockUsbSerialDevice,
    disconnectUsbSerialDevice,
    removeDevice,
    refreshDevice,
    resetDemo,
  } = useDeviceRegistry();
  const demoMode = isDemoSeed(
    new URLSearchParams(window.location.search).get("seed"),
  );
  const [target, setTarget] = useState("");
  const [alias, setAlias] = useState("");
  const [location, setLocation] = useState("");
  const [usbAlias, setUsbAlias] = useState("");
  const [usbLocation, setUsbLocation] = useState("");
  const [fallbackDevdTarget] = useState(() =>
    demoMode
      ? "mock:devd"
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
  const [devdBindTargets, setDevdBindTargets] = useState<
    Record<string, string>
  >({});
  const serialSupported = isWebSerialSupported();
  const devdDiscoveryOnly = hostedHttpServiceApp;
  const devdTarget = sharedDevdDiscovery?.devdTarget ?? fallbackDevdTarget;
  const devdDevices = sharedDevdDiscovery?.devdDevices ?? [];
  const devdStatus = sharedDevdDiscovery?.status ?? "checking";
  const devdLastUpdated = sharedDevdDiscovery?.lastUpdated ?? null;
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
    setBusy(true);
    setMessage(null);
    const result = await addDevice({ target, alias, location });
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

  async function onSavedDeviceChannelSwitch(
    record: DeviceRecord,
    transport: DeviceChannelTransport,
  ) {
    setSavedDeviceSwitchTarget({ deviceId: record.target.deviceId, transport });
    setSavedDeviceMessage(null);
    const result = await connectKnownDeviceChannel(
      record.target.deviceId,
      transport,
    );
    setSavedDeviceSwitchTarget(null);
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
  const showLanFallback = !devdDiscoveryOnly && devdStatus === "unavailable";
  const showFallbackConnectPanels = !devdDiscoveryOnly;
  const devdLastUpdatedLabel = devdLastUpdated
    ? timeAgo(devdLastUpdated)
    : "not yet";

  return (
    <section className="page-flow connect-wide">
      <div className="section-heading">
        <h2>Connect devices</h2>
        <p>
          {devdDiscoveryOnly
            ? "This self-hosted devd UI reads current mains-aegis-devd device records: USB devices attach through devd, while LAN devices connect directly to the hardware HTTP API."
            : "When mains-aegis-devd is reachable, current USB CDC and LAN device records appear here automatically."}
        </p>
      </div>

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
                    <>
                      <button
                        className="primary-button small"
                        type="button"
                        onClick={() =>
                          navigate(deviceDefaultHref(existingRecord))
                        }
                      >
                        Open
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
                    ? "Fallback for direct hardware HTTP/SSE when devd is unreachable"
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
                      disabled={busy}
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
                        disabled: isBusy || isActive,
                      };
                    });
                    return (
                      <div className="channel-switch-actions">
                        <button
                          className="secondary-button small"
                          type="button"
                          disabled={
                            recommendedActive || Boolean(recommendedBusy)
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
          : "Connection failed";
  const guidance =
    code === "serial_port_unavailable"
      ? "Disconnect the devd session or close the app using this CDC port, then retry."
      : code === "firmware_artifact_mismatch"
        ? "Select matching firmware, flash the current build, or explicitly ignore this warning to continue."
        : code === "devd_http_service_requires_devd_panel"
          ? "LAN status connects directly to hardware over the device HTTP API. Use the devd panel only for mains-aegis-devd HTTP service endpoints."
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

function DeviceOverviewPage({ record }: { record: DeviceRecord }) {
  const status = record.status;
  return (
    <section className="page-flow" data-evidence-target="device-overview">
      <DeviceStatusBand record={record} />
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

function PowerPage({ record }: { record: DeviceRecord }) {
  const status = record.status;
  return (
    <section className="page-flow">
      <DeviceStatusBand record={record} />
      <div className="detail-grid three">
        <InfoPanel title="Input" icon={PlugZap}>
          <MetricLine
            label="Mains present"
            value={boolLabel(status?.input.mains_present, "yes", "no")}
          />
          <MetricLine
            label="VIN VBUS"
            value={formatVoltage(status?.input.vin_vbus_mv)}
          />
          <MetricLine
            label="VIN IIN"
            value={formatCurrent(status?.input.vin_iin_ma)}
          />
        </InfoPanel>
        <InfoPanel title="Charger" icon={BatteryCharging}>
          <MetricLine label="State" value={status?.charger.state ?? "--"} />
          <MetricLine
            label="Allow charge"
            value={boolLabel(status?.charger.allow_charge, "yes", "no")}
          />
          <MetricLine
            label="ICHG"
            value={formatCurrent(status?.charger.ichg_ma)}
          />
          <MetricLine
            label="IBAT"
            value={formatCurrent(status?.charger.ibat_ma)}
          />
        </InfoPanel>
        <InfoPanel title="Output gate" icon={Cable}>
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
  );
}

function BatteryPage({ record }: { record: DeviceRecord }) {
  const battery = record.status?.battery;
  const cells = normalizeCellVoltages(battery?.cell_mv);
  const cellModel = buildCellBalanceModel(cells, battery);
  return (
    <section className="page-flow">
      <DeviceStatusBand record={record} />
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
            {battery?.issue_detail ??
              "No active battery issue reported by the v1 status snapshot."}
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
      <DeviceStatusBand record={record} />
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
  return (
    <section className="page-flow">
      <DeviceStatusBand record={record} />
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
      </div>
    </section>
  );
}

function SettingsPage({ record }: { record: DeviceRecord }) {
  const { sendWifiConfig, clearWifiConfig, setManualChargePrefs } =
    useDeviceRegistry();
  const settings = record.settings;
  const [ssid, setSsid] = useState(settings?.wifi.ssid ?? "");
  const [psk, setPsk] = useState("");
  const [manualPrefs, setManualPrefs] = useState<
    DeviceSettings["manual_charge"]
  >(
    settings?.manual_charge ??
      ({
        target: "full_100",
        speed: "ma_500",
        timer_h: 2,
      } as DeviceSettings["manual_charge"]),
  );
  const [message, setMessage] = useState<UiFeedback | null>(null);
  const [wifiMessage, setWifiMessage] = useState<UiFeedback | null>(null);
  const [wifiProgress, setWifiProgress] =
    useState<WifiProvisioningProgress | null>(null);
  const [busy, setBusy] = useState<
    "wifi-save" | "wifi-clear" | "manual" | null
  >(null);
  const activeTransport = activeRecordTransport(record);
  const usbReady = activeTransport === "serial";
  const lanReady = activeTransport === "http" && Boolean(record.settings);
  const devdReady = activeTransport === "devd" && Boolean(record.settings);
  const settingsReady = usbReady || lanReady || devdReady;
  const transportLabel = lanReady && !usbReady ? "LAN" : "hardware";
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
    setManualPrefs(settings.manual_charge);
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

  async function onManualPrefsSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy("manual");
    setMessage(null);
    const result = await setManualChargePrefs(
      record.target.deviceId,
      manualPrefs,
    );
    setBusy(null);
    setMessage(
      result.ok
        ? successFeedback("Manual charge preferences updated")
        : errorFeedback(result.error),
    );
  }

  if (!settingsReady) {
    return (
      <section className="page-flow">
        <DeviceStatusBand record={record} />
        <section className="empty-state">
          <SlidersHorizontal size={28} />
          <h2>Settings unavailable</h2>
          <p>
            Refresh this device or connect USB / mains-aegis-devd before
            changing settings.
          </p>
          <button
            className="primary-button"
            type="button"
            onClick={() => navigate("/connect")}
          >
            Connect device
          </button>
        </section>
      </section>
    );
  }

  return (
    <section className="page-flow" data-evidence-target="wifi-settings">
      <DeviceStatusBand record={record} />
      <div className="settings-layout settings-layout-balanced">
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
            <h2>Device settings</h2>
          </header>
          <form className="settings-form" onSubmit={onManualPrefsSubmit}>
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
            <button
              className="primary-button"
              type="submit"
              disabled={busy !== null}
            >
              <ButtonLabel
                busy={busy === "manual"}
                busyText="Applying"
                text="Apply prefs"
              />
            </button>
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
      <DeviceStatusBand record={record} />
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
}: {
  title: string;
  icon: typeof Gauge;
  children: React.ReactNode;
}) {
  return (
    <section className="info-panel">
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
      detail: "Waiting for the first device response",
      tone: "info",
    };
  }

  if (record.streamState === "error") {
    return {
      label: record.status ? "Live data" : "Connection error",
      detail: record.status
        ? `Transport reconnecting, polling fallback${freshness}`
        : (record.error?.message ?? `Stream error${freshness}`),
      tone: record.status ? "warning" : "critical",
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
      label: "Live",
      detail: `${record.streamState}${freshness}`,
      tone: "ok",
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
}: {
  label: string;
  value: number;
  tone?: "neutral" | "critical" | "warning" | "offline" | "ok";
}) {
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

function MissingDevice() {
  return (
    <section className="empty-state">
      <Server size={28} />
      <h2>Device not found</h2>
      <p>The selected device is no longer in the local registry.</p>
      <button className="primary-button" onClick={() => navigate("/")}>
        Back to fleet
      </button>
    </section>
  );
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
    status.thermal.tmp_a_state === "hot" ||
    status.thermal.tmp_b_state === "hot"
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
