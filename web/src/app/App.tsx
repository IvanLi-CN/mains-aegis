import {
  Activity,
  AlertTriangle,
  BatteryFull,
  BatteryLow,
  BatteryMedium,
  BatteryWarning,
  BatteryCharging,
  Cable,
  Cpu,
  Gauge,
  Globe2,
  LayoutGrid,
  Menu,
  PlugZap,
  RefreshCw,
  Search,
  Server,
  Thermometer,
  Wifi,
  X,
} from "lucide-react";
import { FormEvent, useEffect, useMemo, useState, type SVGProps } from "react";
import type { LucideIcon } from "lucide-react";
import type { DeviceRecord, UpsStatus } from "../api/types";
import { useDeviceRegistry } from "../device-registry/DeviceRegistry";
import { formatCurrent, formatPercent, formatTemp, formatVoltage, timeAgo } from "../utils/format";
import { deviceSeverity, modeLabel, severityRank, type Severity } from "../utils/severity";

type Route = {
  path: string;
  deviceId: string | null;
  section: "fleet" | "connect" | "overview" | "power" | "battery" | "thermal" | "device" | "api";
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
  { id: "api", label: "API", icon: Cable },
] as const;

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
    case "api":
      return <ApiDebugPage record={selected} />;
    default:
      return <DeviceOverviewPage record={selected} />;
  }
}

function useRoute(initialPath?: string): Route {
  const [path, setPath] = useState(initialPath ?? window.location.pathname);

  useEffect(() => {
    if (initialPath) setPath(initialPath);
  }, [initialPath]);

  useEffect(() => {
    const listener = () => setPath(window.location.pathname);
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
  const next = new URL(path, window.location.origin);
  const currentSeed = new URLSearchParams(window.location.search).get("seed");
  if (!next.search && currentSeed) next.searchParams.set("seed", currentSeed);
  window.history.pushState(null, "", `${next.pathname}${next.search}${next.hash}`);
  window.dispatchEvent(new PopStateEvent("popstate"));
}

function deviceHref(deviceId: string, section: string) {
  return section === "overview" ? `/devices/${encodeURIComponent(deviceId)}` : `/devices/${encodeURIComponent(deviceId)}/${section}`;
}

function NavLink({ href, active, icon: Icon, label }: { href: string; active: boolean; icon: LucideIcon; label: string }) {
  return (
    <a
      className={`nav-link ${active ? "is-active" : ""}`}
      href={href}
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
        <div className="segmented" aria-label="Fleet filter">
          {(["all", "critical", "warning", "offline"] as const).map((item) => (
            <button key={item} className={filter === item ? "is-active" : ""} onClick={() => setFilter(item)}>
              {item}
            </button>
          ))}
        </div>
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
  const { records, addDevice, removeDevice, refreshDevice, resetDemo } = useDeviceRegistry();
  const [target, setTarget] = useState("");
  const [alias, setAlias] = useState("");
  const [location, setLocation] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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

  return (
    <section className="page-flow connect-wide">
      <div className="section-heading">
        <h2>Connect devices</h2>
        <p>Add UPS targets by `.local` hostname or IP. The console probes ping, identity, network, and status before saving.</p>
      </div>

      <form className="connect-form" onSubmit={onSubmit}>
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
          <button className="primary-button" type="submit" disabled={busy}>{busy ? "Connecting" : "Add device"}</button>
          <button className="secondary-button" type="button" onClick={resetDemo}>Reset demo fleet</button>
        </div>
      </form>

      {message ? (
        <p className="form-message" role="status" aria-live="polite">
          {message}
        </p>
      ) : null}

      <div className="table-list">
        {records.map((record) => (
          <div className="table-row" key={record.target.deviceId}>
            <div>
              <strong>{record.target.alias}</strong>
              <span>{record.target.baseUrl}</span>
            </div>
            <div className="row-actions">
              <button
                className="icon-button"
                type="button"
                aria-label={`Refresh ${record.target.alias}`}
                title={`Refresh ${record.target.alias}`}
                onClick={() => void refreshDevice(record.target.deviceId)}
              >
                <RefreshCw size={16} />
              </button>
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

function ApiDebugPage({ record }: { record: DeviceRecord }) {
  const payload = {
    identity: record.identity,
    network: record.network,
    status: record.status,
    error: record.error,
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
        </InfoPanel>
        <pre className="json-view">{JSON.stringify(payload, null, 2)}</pre>
      </div>
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

function MetricLine({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric-line">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
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
