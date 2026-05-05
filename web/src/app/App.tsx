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
  Gauge,
  Globe2,
  KeyRound,
  LayoutGrid,
  Maximize2,
  Menu,
  Minimize2,
  PlugZap,
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
import { FormEvent, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type SVGProps } from "react";
import type { LucideIcon } from "lucide-react";
import type { DeviceRecord, SafeSettingsState, SerialLogEntry, SerialTraceEntry, UpsStatus } from "../api/types";
import { SegmentedControl } from "../components/ui/segmented-control";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { useDeviceRegistry } from "../device-registry/context";
import { isDemoSeed } from "../fixtures/mockDevices";
import { isWebSerialSupported } from "../serial/transport";
import { formatCurrent, formatPercent, formatTemp, formatVoltage, timeAgo } from "../utils/format";
import { deviceSeverity, modeLabel, severityRank, type Severity } from "../utils/severity";
import { captureTraceScrollAnchor, resolveAnchoredTraceScrollTop, type TraceScrollAnchor } from "./traceScrollAnchor";

type Route = {
  path: string;
  deviceId: string | null;
  section: "fleet" | "connect" | "overview" | "power" | "battery" | "thermal" | "device" | "settings" | "api";
};

type AppProps = {
  initialPath?: string;
};

const deviceSections = [
  { id: "overview", label: "Overview", icon: Gauge },
  { id: "power", label: "Power", icon: PlugZap },
  { id: "battery", label: "Battery", icon: BatteryCharging },
  { id: "thermal", label: "Thermal", icon: Thermometer },
  { id: "device", label: "Device", icon: Cpu },
  { id: "settings", label: "Settings", icon: Settings },
  { id: "api", label: "API", icon: Cable },
] as const;

const appBasePath = normalizeBasePath(import.meta.env.BASE_URL);
const docsHref = `${appBasePath}docs/`;

export function App({ initialPath }: AppProps = {}) {
  const registry = useDeviceRegistry();
  const route = useRoute(initialPath);
  const selected = route.deviceId ? (registry.records.find((record) => record.target.deviceId === route.deviceId) ?? null) : null;
  const [navOpen, setNavOpen] = useState(false);

  useEffect(() => {
    setNavOpen(false);
  }, [route.path]);

  return (
    <div className="app-shell">
      <aside className={`sidebar ${navOpen ? "is-open" : ""}`}>
        <div className="mobile-nav-bar">
          <button className="icon-button" type="button" aria-label={navOpen ? "Close navigation" : "Open navigation"} onClick={() => setNavOpen((open) => !open)}>
            {navOpen ? <X size={18} /> : <Menu size={18} />}
          </button>
          <div className="mobile-nav-title">
            <strong>Mains Aegis</strong>
            <span>{route.section === "connect" ? "Connect" : selected?.target.alias ?? "Fleet"}</span>
          </div>
        </div>
        <button className="mobile-nav-backdrop" type="button" aria-label="Close navigation" onClick={() => setNavOpen(false)} />
        <div className="sidebar-panel">
          <div className="brand">
            <span className="brand-mark">MA</span>
            <div>
              <strong>Mains Aegis</strong>
              <span>UPS fleet console</span>
            </div>
          </div>

          <nav className="nav-group" aria-label="Fleet navigation">
            <NavLink href="/" active={route.section === "fleet"} icon={LayoutGrid} label="Fleet" />
            <NavLink href="/connect" active={route.section === "connect"} icon={Wifi} label="Connect" />
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

      <main className={`main-surface ${route.section === "connect" ? "connect-adapt-command" : ""}`}>
        <TopBar records={registry.records} selected={selected} />
        {renderRoute(route, registry.records, selected)}
      </main>
    </div>
  );
}

function renderRoute(route: Route, records: DeviceRecord[], selected: DeviceRecord | null) {
  if (route.section === "connect") return <ConnectPage />;
  if (!route.deviceId) return <FleetPage records={records} />;
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
  window.history.pushState(null, "", `${next.pathname}${next.search}${next.hash}`);
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
  if (pathname.startsWith(appBasePath)) return pathname.slice(baseWithoutTrailingSlash.length) || "/";
  return pathname;
}

function withBasePath(path: string): string {
  if (appBasePath === "/") return path;
  const pathname = path.startsWith("/") ? path : `/${path}`;
  return `${appBasePath.slice(0, -1)}${pathname}`;
}

function deviceHref(deviceId: string, section: string) {
  return section === "overview" ? `/devices/${encodeURIComponent(deviceId)}` : `/devices/${encodeURIComponent(deviceId)}/${section}`;
}

function deviceDefaultHref(record: DeviceRecord) {
  return deviceHref(record.target.deviceId, record.target.transport === "serial" || record.target.transport === "adapter" ? "settings" : "overview");
}

function NavLink({ href, active, icon: Icon, label }: { href: string; active: boolean; icon: LucideIcon; label: string }) {
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

function ExternalNavLink({ href, icon: Icon, label }: { href: string; icon: LucideIcon; label: string }) {
  return (
    <a className="nav-link" href={href} target="_blank" rel="noreferrer">
      <Icon size={17} />
      <span>{label}</span>
    </a>
  );
}

function TopBar({ records, selected }: { records: DeviceRecord[]; selected: DeviceRecord | null }) {
  const counts = useMemo(() => {
    const severities = records.map(deviceSeverity);
    return {
      total: records.length,
      online: records.filter((record) => record.connectionState === "online").length,
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
        <Metric label="Critical" value={counts.critical} tone={counts.critical > 0 ? "critical" : "ok"} />
        <Metric label="Warning" value={counts.warning} tone={counts.warning > 0 ? "warning" : "ok"} />
        <Metric label="Offline" value={counts.offline} tone={counts.offline > 0 ? "offline" : "ok"} />
      </div>
    </header>
  );
}

function FleetPage({ records }: { records: DeviceRecord[] }) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Severity | "all">("all");
  const filtered = records
    .filter((record) => {
      const haystack = `${record.target.alias} ${record.target.location} ${record.identity?.hostname ?? record.target.deviceId}`.toLowerCase();
      const matchesQuery = haystack.includes(query.toLowerCase());
      const matchesFilter = filter === "all" || deviceSeverity(record) === filter;
      return matchesQuery && matchesFilter;
    })
    .sort((a, b) => severityRank(deviceSeverity(a)) - severityRank(deviceSeverity(b)) || a.target.alias.localeCompare(b.target.alias));

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

      <div className="fleet-grid" data-evidence-target="fleet-grid">
        {filtered.length > 0 ? filtered.map((record) => (
          <DeviceCard key={record.target.deviceId} record={record} />
        )) : <FleetEmptyState hasDevices={records.length > 0} />}
      </div>
    </section>
  );
}

function FleetEmptyState({ hasDevices }: { hasDevices: boolean }) {
  return (
    <section className="empty-state fleet-empty">
      <Server size={28} />
      <h2>{hasDevices ? "No matching devices" : "No UPS devices saved"}</h2>
      <p>
        {hasDevices
          ? "Adjust the search or status filter to bring devices back into view."
          : "Add a .local hostname, IP address, or mock target from the connect page to build this browser's device registry."}
      </p>
      <button className="primary-button" type="button" onClick={() => navigate("/connect")}>
        Connect devices
      </button>
    </section>
  );
}

function DeviceCard({ record }: { record: DeviceRecord }) {
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
        <span className={`mode-pill mode-${status?.mode ?? "unknown"}`}>{modeLabel(status?.mode)}</span>
        <SeverityBadge severity={severity} />
      </div>

      <div className="card-main card-main-icon-duo metric-duo-stack">
        <div className="metric-tile">
          <span className="metric-icon metric-icon-size-a"><BatteryLevelIcon status={status} /></span>
          <span className="metric-copy">
            <span className="metric-label">Battery</span>
            <strong>{formatPercent(status?.battery.soc_pct)}</strong>
          </span>
        </div>
        <div className="metric-tile">
          <span className="metric-icon metric-icon-size-a"><PowerMetricIcon record={record} /></span>
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
        <span>{record.target.location} · {timeAgo(record.lastUpdated)}</span>
        <div>
          <button
            className="primary-button small"
            type="button"
            aria-label={`Open ${record.target.alias} details`}
            onClick={() => navigate(deviceHref(record.target.deviceId, "overview"))}
          >
            Details
          </button>
        </div>
      </div>
    </article>
  );
}

function ConnectPage() {
  const {
    records,
    addDevice,
    addLocalAdapterDevice,
    connectUsbSerialDevice,
    attachMockUsbSerialDevice,
    disconnectUsbSerialDevice,
    removeDevice,
    refreshDevice,
    resetDemo,
  } = useDeviceRegistry();
  const [target, setTarget] = useState("");
  const [alias, setAlias] = useState("");
  const [location, setLocation] = useState("");
  const [usbAlias, setUsbAlias] = useState("");
  const [usbLocation, setUsbLocation] = useState("");
  const [adapterTarget, setAdapterTarget] = useState("same-origin");
  const [adapterAlias, setAdapterAlias] = useState("");
  const [adapterLocation, setAdapterLocation] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [usbMessage, setUsbMessage] = useState<string | null>(null);
  const [adapterMessage, setAdapterMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [usbBusy, setUsbBusy] = useState(false);
  const [adapterBusy, setAdapterBusy] = useState(false);
  const serialSupported = isWebSerialSupported();
  const demoMode = isDemoSeed(new URLSearchParams(window.location.search).get("seed"));

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
      setMessage(`Connected ${result.record.target.alias}`);
    } else {
      setMessage(`${result.error?.code}: ${result.error?.message}`);
    }
  }

  async function onUsbConnect() {
    setUsbBusy(true);
    setUsbMessage(null);
    const result = await connectUsbSerialDevice({ alias: usbAlias, location: usbLocation });
    setUsbBusy(false);
    if (result.ok) {
      setUsbAlias("");
      setUsbLocation("");
      setUsbMessage(`USB connected ${result.record.target.alias}`);
      navigate(deviceHref(result.record.target.deviceId, "settings"));
    } else {
      setUsbMessage(`${result.error?.code}: ${result.error?.message}`);
    }
  }

  async function onAdapterSubmit(event: FormEvent) {
    event.preventDefault();
    setAdapterBusy(true);
    setAdapterMessage(null);
    const result = await addLocalAdapterDevice({ target: adapterTarget, alias: adapterAlias, location: adapterLocation });
    setAdapterBusy(false);
    if (result.ok) {
      setAdapterAlias("");
      setAdapterLocation("");
      setAdapterMessage(`Local adapter connected ${result.record.target.alias}`);
      navigate(deviceHref(result.record.target.deviceId, "settings"));
    } else {
      setAdapterMessage(`${result.error?.code}: ${result.error?.message}`);
    }
  }

  function onMockUsbConnect() {
    const result = attachMockUsbSerialDevice();
    if (result.ok) {
      setUsbMessage(`USB demo attached ${result.record.target.alias}`);
      navigate(deviceHref(result.record.target.deviceId, "settings"));
    }
  }

  return (
    <section className="page-flow connect-wide">
      <div className="section-heading">
        <h2>Connect devices</h2>
        <p>USB CDC is the control path. LAN targets remain read-only status sources.</p>
      </div>

      <div className="connect-grid" data-evidence-target="usb-connect">
        <section className="connect-panel usb-panel">
          <header className="connect-panel-header">
            <div>
              <h3><Usb size={18} /> USB CDC</h3>
              <p>{serialSupported ? "Chromium Web Serial available" : "Web Serial unavailable in this browser"}</p>
            </div>
            <span className={`transport-badge ${serialSupported ? "serial" : "offline"}`}>{serialSupported ? "ready" : "unsupported"}</span>
          </header>
          <div className="connect-form compact">
            <label>
              Alias
              <input name="usb-alias" value={usbAlias} onChange={(event) => setUsbAlias(event.target.value)} placeholder="Lab bench USB" />
            </label>
            <label>
              Location
              <input name="usb-location" value={usbLocation} onChange={(event) => setUsbLocation(event.target.value)} placeholder="Bench 1" />
            </label>
            <div className="form-actions with-callout">
              <button
                className="primary-button"
                type="button"
                disabled={usbBusy || !serialSupported}
                onClick={() => void onUsbConnect()}
                aria-describedby={usbMessage ? "usb-connect-message" : undefined}
              >
                <Usb size={16} /> {usbBusy ? "Connecting" : "Connect USB"}
              </button>
              {usbMessage ? <ConnectionCallout id="usb-connect-message" message={usbMessage} /> : null}
              {demoMode ? (
                <button className="secondary-button" type="button" onClick={onMockUsbConnect}>
                  <Terminal size={16} /> Mock USB
                </button>
              ) : null}
            </div>
          </div>
        </section>

        <section className="connect-panel adapter-panel">
          <header className="connect-panel-header">
            <div>
              <h3><Server size={18} /> Local USB Adapter</h3>
              <p>Rust HTTP bridge for Web, App, and CLI clients</p>
            </div>
            <span className="transport-badge adapter">localhost</span>
          </header>
          <form className="connect-form compact" onSubmit={onAdapterSubmit}>
            <label>
              Adapter URL
              <input
                name="adapter-target"
                value={adapterTarget}
                onChange={(event) => setAdapterTarget(event.target.value)}
                placeholder="same-origin"
                required
              />
            </label>
            <label>
              Alias
              <input name="adapter-alias" value={adapterAlias} onChange={(event) => setAdapterAlias(event.target.value)} placeholder="Lab bench adapter" />
            </label>
            <label>
              Location
              <input name="adapter-location" value={adapterLocation} onChange={(event) => setAdapterLocation(event.target.value)} placeholder="Bench 1" />
            </label>
            <div className="form-actions">
              <button className="primary-button" type="submit" disabled={adapterBusy}>
                <Server size={16} /> {adapterBusy ? "Connecting" : "Connect Adapter"}
              </button>
            </div>
          </form>
          {adapterMessage ? <p className="form-message" role="status" aria-live="polite">{adapterMessage}</p> : null}
        </section>

        <section className="connect-panel">
          <header className="connect-panel-header">
            <div>
              <h3><Globe2 size={18} /> LAN status</h3>
              <p>HTTP/SSE identity, network, and status probe</p>
            </div>
            <span className="transport-badge http">read-only</span>
          </header>
          <form className="connect-form compact" onSubmit={onSubmit}>
            <label>
              Target
              <input
                name="device-target"
                value={target}
                onChange={(event) => setTarget(event.target.value)}
                placeholder="mains-aegis-a1b2c3.local or 192.168.31.42"
                required
              />
            </label>
            <label>
              Alias
              <input name="device-alias" value={alias} onChange={(event) => setAlias(event.target.value)} placeholder="Lab rack A" />
            </label>
            <label>
              Location
              <input name="device-location" value={location} onChange={(event) => setLocation(event.target.value)} placeholder="Bench 1" />
            </label>
            <div className="form-actions">
              <button className="primary-button" type="submit" disabled={busy}>{busy ? "Connecting" : "Add LAN"}</button>
              {demoMode ? (
                <button className="secondary-button" type="button" onClick={resetDemo}>Reset demo fleet</button>
              ) : null}
            </div>
          </form>
          {message ? (
            <p className="form-message" role="status" aria-live="polite">
              {message}
            </p>
          ) : null}
        </section>
      </div>

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
              <span
                className={`transport-badge ${
                  record.target.transport === "serial" ? "serial" : record.target.transport === "adapter" ? "adapter" : "http"
                }`}
              >
                {record.target.transport === "serial" ? "USB" : record.target.transport === "adapter" ? "Adapter" : "LAN"}
              </span>
              <button
                className="icon-button"
                type="button"
                aria-label={`Refresh ${record.target.alias}`}
                title={`Refresh ${record.target.alias}`}
                onClick={() => void refreshDevice(record.target.deviceId)}
              >
                <RefreshCw size={16} />
              </button>
              {record.target.transport === "serial" && record.serial?.connected ? (
                <button
                  className="icon-button"
                  type="button"
                  aria-label={`Disconnect ${record.target.alias}`}
                  title={`Disconnect ${record.target.alias}`}
                  onClick={() => void disconnectUsbSerialDevice(record.target.deviceId)}
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
    </section>
  );
}

function ConnectionCallout({ id, message }: { id: string; message: string }) {
  const [code, body] = splitErrorMessage(message);
  const title = code === "serial_port_unavailable" ? "USB port is in use" : "Connection failed";
  const guidance =
    code === "serial_port_unavailable"
      ? "Disconnect the devd adapter session or close the app using this CDC port, then retry."
      : "Check the selected device and try again.";

  return (
    <aside id={id} className="connection-callout" role="status" aria-live="polite">
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

function DeviceOverviewPage({ record }: { record: DeviceRecord }) {
  const status = record.status;
  return (
    <section className="page-flow" data-evidence-target="device-overview">
      <DeviceStatusBand record={record} />
      <div className="detail-grid">
        <InfoPanel title="Input" icon={PlugZap}>
          <MetricLine label="Mains" value={boolLabel(status?.input.mains_present, "present", "absent")} />
          <MetricLine label="VBUS" value={formatVoltage(status?.input.input_vbus_mv)} />
          <MetricLine label="IBUS" value={formatCurrent(status?.input.input_ibus_ma)} />
        </InfoPanel>
        <InfoPanel title="Battery" icon={BatteryCharging}>
          <MetricLine label="SOC" value={formatPercent(status?.battery.soc_pct)} />
          <MetricLine label="Pack" value={formatVoltage(status?.battery.pack_mv)} />
          <MetricLine label="Ready" value={boolLabel(status?.battery.discharge_ready, "yes", "no")} />
        </InfoPanel>
        <InfoPanel title="Outputs" icon={Cable}>
          <MetricLine label="Active" value={status?.output.active ?? "--"} />
          <MetricLine label="OUT A" value={channelLabel(status?.output.out_a)} />
          <MetricLine label="OUT B" value={channelLabel(status?.output.out_b)} />
        </InfoPanel>
        <InfoPanel title="Thermal" icon={Thermometer}>
          <MetricLine label="TMP A" value={`${formatTemp(status?.thermal.tmp_a_c)} / ${status?.thermal.tmp_a_state ?? "--"}`} />
          <MetricLine label="TMP B" value={`${formatTemp(status?.thermal.tmp_b_c)} / ${status?.thermal.tmp_b_state ?? "--"}`} />
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
          <MetricLine label="Mains present" value={boolLabel(status?.input.mains_present, "yes", "no")} />
          <MetricLine label="VIN VBUS" value={formatVoltage(status?.input.vin_vbus_mv)} />
          <MetricLine label="VIN IIN" value={formatCurrent(status?.input.vin_iin_ma)} />
        </InfoPanel>
        <InfoPanel title="Charger" icon={BatteryCharging}>
          <MetricLine label="State" value={status?.charger.state ?? "--"} />
          <MetricLine label="Allow charge" value={boolLabel(status?.charger.allow_charge, "yes", "no")} />
          <MetricLine label="ICHG" value={formatCurrent(status?.charger.ichg_ma)} />
          <MetricLine label="IBAT" value={formatCurrent(status?.charger.ibat_ma)} />
        </InfoPanel>
        <InfoPanel title="Output gate" icon={Cable}>
          <MetricLine label="Requested" value={status?.output.requested ?? "--"} />
          <MetricLine label="Active" value={status?.output.active ?? "--"} />
          <MetricLine label="Recoverable" value={status?.output.recoverable ?? "--"} />
          <MetricLine label="Gate reason" value={status?.output.gate_reason ?? "none"} />
        </InfoPanel>
        <InfoPanel title="OUT A" icon={Activity}>
          <MetricLine label="State" value={status?.output.out_a.state ?? "--"} />
          <MetricLine label="Enabled" value={boolLabel(status?.output.out_a.enabled, "yes", "no")} />
          <MetricLine label="Voltage" value={formatVoltage(status?.output.out_a.vbus_mv)} />
          <MetricLine label="Current" value={formatCurrent(status?.output.out_a.iout_ma)} />
        </InfoPanel>
        <InfoPanel title="OUT B" icon={Activity}>
          <MetricLine label="State" value={status?.output.out_b.state ?? "--"} />
          <MetricLine label="Enabled" value={boolLabel(status?.output.out_b.enabled, "yes", "no")} />
          <MetricLine label="Voltage" value={formatVoltage(status?.output.out_b.vbus_mv)} />
          <MetricLine label="Current" value={formatCurrent(status?.output.out_b.iout_ma)} />
        </InfoPanel>
      </div>
    </section>
  );
}

function BatteryPage({ record }: { record: DeviceRecord }) {
  const battery = record.status?.battery;
  return (
    <section className="page-flow">
      <DeviceStatusBand record={record} />
      <div className="detail-grid">
        <InfoPanel title="Pack status" icon={BatteryCharging}>
          <MetricLine label="State" value={battery?.state ?? "--"} />
          <MetricLine label="SOC" value={formatPercent(battery?.soc_pct)} />
          <MetricLine label="Pack voltage" value={formatVoltage(battery?.pack_mv)} />
          <MetricLine label="Current" value={formatCurrent(battery?.current_ma)} />
        </InfoPanel>
        <InfoPanel title="BMS readiness" icon={Cpu}>
          <MetricLine label="No battery" value={boolLabel(battery?.no_battery, "yes", "no")} />
          <MetricLine label="Discharge ready" value={boolLabel(battery?.discharge_ready, "yes", "no")} />
          <MetricLine label="Recovery pending" value={boolLabel(battery?.recovery_pending, "yes", "no")} />
          <MetricLine label="Last result" value={battery?.last_result ?? "--"} />
        </InfoPanel>
        <InfoPanel title="Issue detail" icon={AlertTriangle}>
          <p className="panel-note">{battery?.issue_detail ?? "No active battery issue reported by the v1 status snapshot."}</p>
        </InfoPanel>
      </div>
    </section>
  );
}

function ThermalPage({ record }: { record: DeviceRecord }) {
  const thermal = record.status?.thermal;
  return (
    <section className="page-flow">
      <DeviceStatusBand record={record} />
      <div className="detail-grid">
        <InfoPanel title="TMP A" icon={Thermometer}>
          <MetricLine label="State" value={thermal?.tmp_a_state ?? "--"} />
          <MetricLine label="Temperature" value={formatTemp(thermal?.tmp_a_c)} />
        </InfoPanel>
        <InfoPanel title="TMP B" icon={Thermometer}>
          <MetricLine label="State" value={thermal?.tmp_b_state ?? "--"} />
          <MetricLine label="Temperature" value={formatTemp(thermal?.tmp_b_c)} />
        </InfoPanel>
        <InfoPanel title="Protection context" icon={AlertTriangle}>
          <MetricLine label="Output gate" value={record.status?.output.gate_reason ?? "none"} />
          <MetricLine label="Charger" value={record.status?.charger.state ?? "--"} />
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
          <MetricLine label="Device ID" value={identity?.device_id ?? record.target.deviceId} />
          <MetricLine label="Hostname" value={identity?.hostname ?? "--"} />
          <MetricLine label="FQDN" value={identity?.hostname_fqdn ?? "--"} />
          <MetricLine label="API" value={identity?.api_version ?? "--"} />
        </InfoPanel>
        <InfoPanel title="Network" icon={Globe2}>
          <MetricLine label="State" value={network?.state ?? "--"} />
          <MetricLine label="IPv4" value={network?.ipv4 ?? "--"} />
          <MetricLine label="Gateway" value={network?.gateway ?? "--"} />
          <MetricLine label="RSSI" value={network?.rssi_dbm ? `${network.rssi_dbm} dBm` : "--"} />
        </InfoPanel>
        <InfoPanel title="Firmware" icon={Cpu}>
          <MetricLine label="Version" value={identity?.firmware.package_version ?? "--"} />
          <MetricLine label="Profile" value={identity?.firmware.build_profile ?? "--"} />
          <MetricLine label="Build" value={identity?.firmware.build_id ?? "--"} />
          <MetricLine label="Git" value={identity?.firmware.git_sha ?? "--"} />
        </InfoPanel>
      </div>
    </section>
  );
}

function SettingsPage({ record }: { record: DeviceRecord }) {
  const { sendWifiConfig, clearWifiConfig, setManualChargePrefs } = useDeviceRegistry();
  const settings = record.serial?.safeSettings;
  const [ssid, setSsid] = useState(settings?.wifi_ssid ?? "");
  const [psk, setPsk] = useState("");
  const [manualPrefs, setManualPrefs] = useState<SafeSettingsState["manual_charge"]>(
    settings?.manual_charge ?? { target: "full_100", speed: "ma_500", timer_h: 2 },
  );
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const usbReady = (record.target.transport === "serial" || record.target.transport === "adapter") && Boolean(record.serial?.connected);

  useEffect(() => {
    if (!settings) return;
    setManualPrefs(settings.manual_charge);
    if (settings.wifi_ssid) setSsid(settings.wifi_ssid);
  }, [settings]);

  async function onWifiSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setMessage(null);
    const result = await sendWifiConfig(record.target.deviceId, { ssid, psk });
    setBusy(false);
    setPsk("");
    setMessage(result.ok ? `WiFi credentials saved for ${ssid}` : `${result.error?.code}: ${result.error?.message}`);
  }

  async function onWifiClear() {
    setBusy(true);
    setMessage(null);
    const result = await clearWifiConfig(record.target.deviceId);
    setBusy(false);
    if (result.ok) {
      setSsid("");
      setPsk("");
      setMessage("WiFi credentials cleared");
    } else {
      setMessage(`${result.error?.code}: ${result.error?.message}`);
    }
  }

  async function onManualPrefsSubmit(event: FormEvent) {
    event.preventDefault();
    const result = await setManualChargePrefs(record.target.deviceId, manualPrefs);
    setMessage(result.ok ? "Manual charge preferences updated" : `${result.error?.code}: ${result.error?.message}`);
  }

  if (!usbReady) {
    return (
      <section className="page-flow">
        <DeviceStatusBand record={record} />
        <section className="empty-state">
          <Usb size={28} />
          <h2>USB control path required</h2>
          <p>Safe settings and WiFi provisioning require Web Serial or the local USB HTTP adapter.</p>
          <button className="primary-button" type="button" onClick={() => navigate("/connect")}>
            Connect USB
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
              <input name="wifi-ssid" value={ssid} onChange={(event) => setSsid(event.target.value)} maxLength={32} required />
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
                required
              />
            </label>
            <div className="secret-note"><KeyRound size={15} /> PSK is written over USB and cleared from the form after submit.</div>
            <div className="form-actions">
              <button className="primary-button" type="submit" disabled={busy || !ssid || psk.length < 8}>
                Save WiFi
              </button>
              <button className="secondary-button" type="button" onClick={() => void onWifiClear()} disabled={busy}>
                <Trash2 size={16} /> Clear
              </button>
            </div>
          </form>
        </section>

        <section className="info-panel settings-panel">
          <header>
            <SlidersHorizontal size={18} />
            <h2>Safe settings</h2>
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
              onChange={(target) => setManualPrefs((current) => ({ ...current, target: target as SafeSettingsState["manual_charge"]["target"] }))}
            />
            <SettingsSegmentedControl
              label="Charge speed"
              value={manualPrefs.speed}
              options={[
                ["ma_100", "100mA"],
                ["ma_500", "500mA"],
                ["ma_1000", "1A"],
              ]}
              onChange={(speed) => setManualPrefs((current) => ({ ...current, speed: speed as SafeSettingsState["manual_charge"]["speed"] }))}
            />
            <SettingsSegmentedControl
              label="Timer"
              value={String(manualPrefs.timer_h)}
              options={[
                ["1", "1h"],
                ["2", "2h"],
                ["6", "6h"],
              ]}
              onChange={(timer) => setManualPrefs((current) => ({ ...current, timer_h: Number(timer) as 1 | 2 | 6 }))}
            />
            <button className="primary-button" type="submit">Apply prefs</button>
          </form>
        </section>

      </div>
      <UsbDeveloperConsole logs={record.serial?.logs ?? []} trace={record.serial?.trace ?? []} />
      {message ? <p className="form-message" role="status" aria-live="polite">{message}</p> : null}
    </section>
  );
}

function ApiDebugPage({ record }: { record: DeviceRecord }) {
  const payload = {
    identity: record.identity,
    network: record.network,
    status: record.status,
    error: record.error,
    serial: record.serial
      ? {
          connected: record.serial.connected,
          protocol: record.serial.protocol,
          safeSettings: record.serial.safeSettings,
        }
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
          <MetricLine label="USB CDC" value={record.serial?.connected ? "JSONL frames" : "not connected"} />
        </InfoPanel>
        <pre className="json-view">{JSON.stringify(payload, null, 2)}</pre>
      </div>
      {record.serial ? <UsbDeveloperConsole logs={record.serial.logs} trace={record.serial.trace} /> : null}
    </section>
  );
}

function DeviceStatusBand({ record }: { record: DeviceRecord }) {
  const status = record.status;
  const severity = deviceSeverity(record);
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
        <strong className="live-value">{formatPercent(status?.battery.soc_pct)}</strong>
      </div>
      <div className="live-cell">
        <span className="eyebrow">Stream</span>
        <strong className="live-value">{record.streamState}</strong>
      </div>
      <span className={`severity-badge live-state severity-${severity}`}>{severity}</span>
    </div>
  );
}

function InfoPanel({ title, icon: Icon, children }: { title: string; icon: typeof Gauge; children: React.ReactNode }) {
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

function Metric({ label, value, tone = "neutral" }: { label: string; value: number; tone?: "neutral" | "critical" | "warning" | "offline" | "ok" }) {
  return (
    <div className={`top-metric tone-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function MetricLine({ label, value, title }: { label: string; value: string; title?: string }) {
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
      <SegmentedControl label={label} value={value} options={options} onChange={onChange} variant="compact" />
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

function traceEntryLevel(entry: SerialTraceEntry): Exclude<TraceLevelFilter, "all"> {
  if (entry.frameType === "error") return "error";
  if (entry.kind === "ignored" && entry.frameType === "defmt") return "warn";
  if (entry.kind === "ignored") return "trace";
  const bracketLevel = entry.payload.match(/^\[(ERROR|WARN|INFO|DEBUG|TRACE)\s*\]/i)?.[1]?.toLowerCase();
  if (bracketLevel && bracketLevel in traceLevelRank) return bracketLevel as Exclude<TraceLevelFilter, "all">;
  try {
    const parsed = JSON.parse(entry.payload) as { level?: unknown };
    if (typeof parsed.level === "string" && parsed.level in traceLevelRank) return parsed.level as Exclude<TraceLevelFilter, "all">;
  } catch {
    // Payloads can be plain boot logs or legacy console text.
  }
  if (entry.kind === "raw") return "debug";
  return "info";
}

function traceSearchText(entry: SerialTraceEntry, level: string) {
  return [entry.direction, level, entry.kind, entry.frameType, entry.requestId, entry.target, entry.summary, entry.payload].filter(Boolean).join(" ").toLowerCase();
}

type DefmtDecodeStatus = {
  label: string;
  tone: "ok" | "warn" | "muted";
  detail: string;
};

function defmtDecodeStatus(trace: SerialTraceEntry[]): DefmtDecodeStatus {
  const defmtEntries = trace.filter((entry) => entry.frameType === "defmt");
  const decodeIssues = defmtEntries.filter((entry) => entry.kind === "ignored");
  const decoded = defmtEntries.filter((entry) => entry.kind !== "ignored" && entry.summary.trim().length > 0);
  if (decodeIssues.length > 0) {
    const latestIssue = decodeIssues[decodeIssues.length - 1];
    const issueText = `${latestIssue.summary} ${latestIssue.payload}`.toLowerCase();
    if (issueText.includes("elf") || issueText.includes("artifact") || issueText.includes("metadata")) {
      return {
        label: "defmt artifact issue",
        tone: "warn",
        detail: "Binary defmt frames are arriving, but the selected firmware artifact or metadata does not match this device.",
      };
    }
    if (issueText.includes("server") || issueText.includes("api") || issueText.includes("proxy") || issueText.includes("decode")) {
      return {
        label: "defmt decoder issue",
        tone: "warn",
        detail: "Binary defmt frames are arriving, but the decoder API cannot decode them. Check devd, the proxy, and the selected firmware artifact.",
      };
    }
    return {
      label: "defmt decode issue",
      tone: "warn",
      detail: "Binary defmt frames are arriving, but some frames cannot be decoded. Check firmware identity and catalog metadata.",
    };
  }
  if (decoded.length > 0) {
    return {
      label: "defmt decoded",
      tone: "ok",
      detail: "defmt frames are being decoded with the current firmware metadata.",
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
      <button type="button" className="trace-help-trigger" aria-label={`USB console help: ${status.label}`}>
        <CircleHelp size={15} strokeWidth={1.9} />
      </button>
      <span className="trace-help-popover" aria-hidden="true">
        <strong>{status.label}</strong>
        <span>{status.detail}</span>
        <span>Raw shows decoded defmt text when possible. Parsed hides original payloads. Compare shows both.</span>
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
  const lead = message.slice(0, leadEnd).trim() || message.split(/\s+/).slice(0, 2).join(" ");
  return { lead, fields };
}

function traceSummaryLabel(entry: SerialTraceEntry): string {
  if (entry.kind === "ignored" && entry.frameType === "defmt") return "defmt decode issue";
  if (entry.kind !== "frame" && entry.frameType === "defmt") return parseTraceMessage(entry.summary).lead;
  return entry.summary;
}

function TraceMessage({ entry, query, mode }: { entry: SerialTraceEntry; query: string; mode: "summary" | "raw" }) {
  if (mode !== "raw" && entry.kind === "ignored" && entry.frameType === "defmt") {
    return (
      <div className="trace-message-readable trace-message-diagnostic">
        <p className="trace-message-lead"><HighlightText value={entry.summary} query={query} /></p>
        <p>Binary CDC data was captured, but the selected defmt decoder could not decode this frame.</p>
      </div>
    );
  }
  if (mode === "raw" || entry.kind === "frame" || entry.frameType !== "defmt") {
    return <HighlightText value={mode === "raw" ? entry.payload : entry.summary} query={query} />;
  }
  const parsed = parseTraceMessage(entry.summary);
  return (
    <div className="trace-message-readable">
      <p className="trace-message-lead"><HighlightText value={parsed.lead} query={query} /></p>
      {parsed.fields.length > 0 ? (
        <dl className="trace-field-list">
          {parsed.fields.map((field, index) => (
            <div className="trace-field" key={`${entry.id}-${field.key}-${index}`}>
              <dt><HighlightText value={field.key} query={query} /></dt>
              <dd><HighlightText value={field.value} query={query} /></dd>
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
    parts.push(<mark key={`${matchIndex}-${cursor}`}>{value.slice(matchIndex, matchIndex + needle.length)}</mark>);
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
      <SegmentedControl label={label} value={value} options={options} onChange={onChange} variant="quiet" />
      <TraceSelectControl label={label} value={value} options={options} onChange={onChange} />
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
      <Select value={value} onValueChange={(nextValue) => onChange(nextValue as T)}>
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

function UsbDeveloperConsole({ logs, trace }: { logs: SerialLogEntry[]; trace: SerialTraceEntry[] }) {
  const [expanded, setExpanded] = useState(false);
  const [wrapLines, setWrapLines] = useState(true);
  const [traceMode, setTraceMode] = useState<"raw" | "parsed" | "compare">("compare");
  const [levelFilter, setLevelFilter] = useState<TraceLevelFilter>("all");
  const [directionFilter, setDirectionFilter] = useState<TraceDirectionFilter>("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [traceScrollTop, setTraceScrollTop] = useState(0);
  const [traceViewportHeight, setTraceViewportHeight] = useState(720);
  const [measuredTraceHeights, setMeasuredTraceHeights] = useState<Record<string, number>>({});
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
        const matchesLevel = levelFilter === "all" || traceLevelRank[level] <= traceLevelRank[levelFilter];
        const matchesDirection = directionFilter === "all" || entry.direction === directionFilter;
        const matchesSearch = !normalizedQuery || traceSearchText(entry, level).includes(normalizedQuery);
        return matchesLevel && matchesDirection && matchesSearch;
      }),
    [directionFilter, levelFilter, normalizedQuery, trace],
  );
  const estimatedTraceHeight = traceMode === "compare" ? (wrapLines ? 128 : 112) : wrapLines ? 72 : 64;
  const traceHeightKey = (entry: SerialTraceEntry) => `${traceMode}:${wrapLines ? "wrap" : "nowrap"}:${entry.id}`;
  const traceLayout = useMemo(() => {
    const offsets: number[] = [];
    let totalHeight = 0;
    for (const entry of filteredTrace) {
      offsets.push(totalHeight);
      totalHeight += measuredTraceHeights[traceHeightKey(entry)] ?? estimatedTraceHeight;
    }
    return { offsets, totalHeight };
  }, [estimatedTraceHeight, filteredTrace, measuredTraceHeights, traceMode, wrapLines]);
  const overscanPx = estimatedTraceHeight * 8;
  const virtualTop = Math.max(0, traceScrollTop - overscanPx);
  const virtualBottom = traceScrollTop + traceViewportHeight + overscanPx;
  let virtualStart = 0;
  while (
    virtualStart < filteredTrace.length &&
    traceLayout.offsets[virtualStart] + (measuredTraceHeights[traceHeightKey(filteredTrace[virtualStart])] ?? estimatedTraceHeight) < virtualTop
  ) {
    virtualStart += 1;
  }
  let virtualEnd = virtualStart;
  while (virtualEnd < filteredTrace.length && traceLayout.offsets[virtualEnd] < virtualBottom) {
    virtualEnd += 1;
  }
  const virtualTrace = filteredTrace.slice(virtualStart, virtualEnd);
  function captureTraceAnchor(scrollTop: number) {
    traceAnchorRef.current = captureTraceScrollAnchor(filteredTrace, traceLayout.offsets, scrollTop);
  }

  function scrollTraceToBottom() {
    const panel = tracePanelRef.current;
    const maxScrollTop = panel ? Math.max(0, panel.scrollHeight - panel.clientHeight) : Math.max(0, traceLayout.totalHeight - traceViewportHeight);
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
    const maxScrollTop = Math.max(0, traceLayout.totalHeight - traceViewportHeight);
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
  }, [filteredTrace, traceLayout, tracePinnedToBottom, traceScrollTop, traceViewportHeight]);

  useEffect(() => {
    const panel = tracePanelRef.current;
    if (!panel) return;
    const updateViewportHeight = () => setTraceViewportHeight(panel.clientHeight || 720);
    updateViewportHeight();
    const observer = new ResizeObserver(updateViewportHeight);
    observer.observe(panel);
    return () => observer.disconnect();
  }, [expanded]);

  function measureTraceItem(entry: SerialTraceEntry, node: HTMLDivElement | null) {
    if (!node) return;
    const key = traceHeightKey(entry);
    const measuredHeight = Math.ceil(node.getBoundingClientRect().height);
    if (!measuredHeight) return;
    if (measuredTraceHeights[key] === measuredHeight) return;
    setMeasuredTraceHeights((current) => (current[key] === measuredHeight ? current : { ...current, [key]: measuredHeight }));
  }

  const renderRawRow = (entry: SerialTraceEntry, key: string, className = `trace-row kind-${entry.kind}`) => {
    const hasDistinctPayload = entry.kind === "frame" || entry.payload !== entry.summary;
    return (
      <div className={className} key={key}>
        <span>{new Date(entry.timestamp).toLocaleTimeString()}</span>
        <strong>{entry.direction}</strong>
        <code>raw</code>
        <em>{entry.requestId ?? entry.target ?? "--"}</em>
        <p className={hasDistinctPayload ? "" : "trace-message-inline"}>
          <HighlightText value={hasDistinctPayload && entry.kind === "frame" ? "raw JSONL frame" : entry.summary} query={searchQuery} />
        </p>
        {hasDistinctPayload ? (
          <pre><TraceMessage entry={entry} query={searchQuery} mode="raw" /></pre>
        ) : null}
      </div>
    );
  };
  const renderParsedRow = (entry: SerialTraceEntry, key: string, className = `trace-row kind-${entry.kind}`) => (
    <div className={className} key={key}>
      <span>{new Date(entry.timestamp).toLocaleTimeString()}</span>
      <strong>{entry.direction}</strong>
      <code>{entry.frameType ?? entry.kind}</code>
      <em>{entry.requestId ?? entry.target ?? "--"}</em>
      <p>
        {entry.kind === "ignored" && entry.frameType === "defmt" ? <b className="trace-diagnostic-tag">Decode issue</b> : null}
        <HighlightText value={traceSummaryLabel(entry)} query={searchQuery} />
      </p>
      <div className="trace-row-body">
        {entry.kind === "frame" ? (
          <HighlightText value={`${entry.frameType ?? "frame"} ${entry.requestId ?? entry.target ?? ""}`.trim()} query={searchQuery} />
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
        {renderRawRow(entry, `${entry.id}-raw`, "trace-row trace-row-original kind-raw")}
      </div>
    );
  };

  return (
    <section className={`info-panel developer-console ${expanded ? "is-expanded" : ""} ${wrapLines ? "wrap-lines" : "no-wrap-lines"}`} data-evidence-target="usb-developer-console">
      <header className="developer-console-header">
        <div className="developer-console-title">
          <Terminal size={18} />
          <h2>USB Console</h2>
          <TraceHelpBubble status={decodeStatus} />
        </div>
        <div className="developer-console-actions">
          <TraceSelectControl label="View" value={traceMode} options={traceModeOptions} onChange={setTraceMode} className="trace-mode-select" />
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
                parsed: "Show human-readable defmt fields and hide the original payload.",
                compare: "Show the parsed view together with the original payload for debugging.",
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
            <input type="search" value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} placeholder="Search logs" aria-label="Search USB console logs" />
          </label>
          <span className="trace-filter-count">{filteredTrace.length} shown</span>
          <div className="developer-console-ops">
            <button className={`trace-live-button ${tracePinnedToBottom ? "is-following" : ""}`} type="button" onClick={scrollTraceToBottom} aria-pressed={tracePinnedToBottom} title={tracePinnedToBottom ? "The console is following new records." : "Jump back to the newest record and follow live updates."}>
              {tracePinnedToBottom ? "Following latest" : "Resume live"}
            </button>
            <label className="switch-control">
              <input type="checkbox" checked={wrapLines} onChange={(event) => setWrapLines(event.target.checked)} />
              <span>Wrap lines</span>
            </label>
            <button className="icon-button" type="button" onClick={() => setExpanded((current) => !current)} aria-label={expanded ? "Exit fullscreen console" : "Open fullscreen console"} title={expanded ? "Exit fullscreen" : "Fullscreen"}>
              {expanded ? <Minimize2 size={16} /> : <Maximize2 size={16} />}
            </button>
          </div>
        </div>
      </header>
      <div className="developer-console-metrics">
        <MetricLine label="CDC records" value={String(trace.length)} title="All USB CDC records captured in the current in-memory trace window." />
        <MetricLine label="Protocol frames" value={String(protocolFrames)} title="Structured command or response frames recognized by the app protocol parser." />
        <MetricLine label="Structured logs" value={String(logs.length)} title="Application log entries that were parsed into structured state." />
        <MetricLine label="Raw / ignored" value={String(rawLines)} title="Records that are not app protocol frames. This can include decoded defmt lines, plain text, or ignored binary payloads." />
      </div>
      <div
        className="trace-panel is-virtualized"
        ref={tracePanelRef}
        role="log"
        aria-label="USB CDC trace records"
        aria-live="off"
        onScroll={(event) => {
          const panel = event.currentTarget;
          const maxScrollTop = Math.max(0, panel.scrollHeight - panel.clientHeight);
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
          <div className="trace-virtual-spacer" style={{ height: traceLayout.totalHeight }}>
            {virtualTrace.map((entry, index) => (
              <div
                className="trace-virtual-item"
                key={entry.id}
                ref={(node) => measureTraceItem(entry, node)}
                style={{ transform: `translateY(${traceLayout.offsets[virtualStart + index]}px)` }}
              >
                {renderTraceEntry(entry)}
              </div>
            ))}
          </div>
        ) : (
          <p className="panel-note">No CDC records match the current filters.</p>
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

function BatteryLevelIcon({ status }: { status: UpsStatus | null | undefined }) {
  const soc = status?.battery.soc_pct;
  const Icon = soc === null || soc === undefined ? BatteryWarning : soc < 20 ? BatteryWarning : soc < 45 ? BatteryLow : soc < 75 ? BatteryMedium : BatteryFull;
  return <Icon size={18} aria-hidden="true" />;
}

function PowerMetricIcon({ record }: { record: DeviceRecord }) {
  const source = powerSourceLabel(record);
  if (source === "Battery") return <BatteryBackupIcon size={18} aria-hidden="true" />;
  const Icon = source === "Offline" || source === "No mains" ? AlertTriangle : PlugZap;
  return <Icon size={18} aria-hidden="true" />;
}

function BatteryBackupIcon({ size = 18, ...props }: SVGProps<SVGSVGElement> & { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" {...props}>
      <rect x="3" y="7" width="16" height="10" rx="2.4" fill="currentColor" />
      <rect x="20" y="10" width="2" height="4" rx="0.8" fill="currentColor" />
    </svg>
  );
}

function SeverityBadge({ severity }: { severity: Severity }) {
  return <span className={`severity-badge severity-${severity}`}>{severity}</span>;
}

function MissingDevice() {
  return (
    <section className="empty-state">
      <Server size={28} />
      <h2>Device not found</h2>
      <p>The selected device is no longer in the local registry.</p>
      <button className="primary-button" onClick={() => navigate("/")}>Back to fleet</button>
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
  if (status.output.active === "out_a" || status.output.active === "out_b") return "Partially powered";
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
  if (status.output.gate_reason && status.output.gate_reason !== "none") return "Protection active";
  if (status.thermal.tmp_a_state === "hot" || status.thermal.tmp_b_state === "hot") return "High temperature";
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
  if (record.network?.state === "connected" || record.status?.network.state === "connected") return "Online";
  return "Check connection";
}

function connectionEndpointLabel(record: DeviceRecord): string {
  if (record.target.transport === "serial") {
    return `${record.target.serialProtocol ?? record.serial?.protocol ?? "USB CDC"} · ${record.serial?.connected ? "connected" : "disconnected"}`;
  }
  if (record.target.transport === "adapter") {
    return `${record.target.baseUrl} · USB adapter ${record.serial?.connected ? "connected" : "disconnected"}`;
  }
  return record.target.baseUrl;
}

function splitErrorMessage(message: string): [string | null, string] {
  const separator = message.indexOf(":");
  if (separator === -1) return [null, message];
  return [message.slice(0, separator).trim(), message.slice(separator + 1).trim()];
}

function channelLabel(channel: UpsStatus["output"]["out_a"] | undefined): string {
  if (!channel) return "--";
  return `${channel.enabled ? "on" : "off"} / ${channel.state}`;
}

function boolLabel(value: boolean | null | undefined, trueLabel: string, falseLabel: string): string {
  if (value === true) return trueLabel;
  if (value === false) return falseLabel;
  return "--";
}

function maxTemp(status: UpsStatus | null | undefined): number | null {
  const values = [status?.thermal.tmp_a_c, status?.thermal.tmp_b_c].filter((value): value is number => typeof value === "number");
  if (values.length === 0) return null;
  return Math.max(...values);
}
