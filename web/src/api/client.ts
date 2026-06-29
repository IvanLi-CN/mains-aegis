import { getMockIdentity, getMockNetwork, getMockStatus } from "../fixtures/mockDevices";
import type {
  AdvancedPowerSettings,
  ApiErrorEnvelope,
  DevdScanTraceEntry,
  DevdDevice,
  DefmtDecodeResult,
  FirmwareCatalog,
  Identity,
  NetworkSummary,
  ProbeResult,
  DeviceSettings,
  SerialLogEntry,
  SerialTraceEntry,
  DevdWebLease,
  UpsStatus,
  WifiApplyNetwork,
} from "./types";

export class MainsAegisApiError extends Error {
  envelope: ApiErrorEnvelope["error"];

  constructor(envelope: ApiErrorEnvelope["error"]) {
    super(envelope.message);
    this.name = "MainsAegisApiError";
    this.envelope = envelope;
  }
}

type MockBindTargetState = {
  boundLogicalDeviceId: string | null;
  dismissedCompanion: boolean;
};

const mockBindTargetStateByBaseUrl = new Map<string, MockBindTargetState>();
const mockSettingsByBaseUrl = new Map<string, DeviceSettings>();

export const isMockBaseUrl = (baseUrl: string) => baseUrl.startsWith("mock:");
const APP_SESSION_HEADER = "x-mains-aegis-app-session";
const APP_SESSION_QUERY_PARAM = "app_session";
const HTTP_SERVICE_MODE_META = 'meta[name="mains-aegis-http-service-mode"]';
const APP_SESSION_META = 'meta[name="mains-aegis-app-session"]';
const RUNTIME_PLACEHOLDER_PREFIX = "__MAINS_AEGIS_";

export type BridgeBootstrap = {
  token_required?: boolean;
  agent_base_url?: string;
  app?: {
    name?: string;
    version?: string;
    mode?: string;
  };
};

export function normalizeBaseUrl(input: string): string {
  const value = input.trim();
  if (value === "" || value === "same-origin" || value === "devd") return "";
  if (value.startsWith("/")) return "";
  if (value.startsWith("mock:")) return value;
  if (/^https?:\/\//i.test(value)) return value.replace(/\/+$/, "");
  return `http://${value.replace(/\/+$/, "")}`;
}

type RequestOptions = {
  bridgeAuth?: boolean;
};

async function requestJson<T>(
  baseUrl: string,
  path: string,
  options?: RequestOptions,
): Promise<T> {
  return requestWithBody<T>(baseUrl, path, "GET", undefined, options);
}

async function requestWithBody<T>(
  baseUrl: string,
  path: string,
  method: "GET" | "POST" | "DELETE",
  body?: unknown,
  options: RequestOptions = {},
): Promise<T> {
  if (isMockBaseUrl(baseUrl)) {
    return requestMock<T>(baseUrl, path, method, body);
  }

  const response = await fetch(`${baseUrl}${path}`, {
    method,
    headers: {
      Accept: "application/json",
      ...(options.bridgeAuth ? bridgeAuthHeaders(baseUrl) : {}),
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

  const responseText = await response.text();
  const payload = parseJsonPayload<T>(responseText);

  if (!response.ok) {
    if (payload && typeof payload === "object" && "error" in payload) {
      throw new MainsAegisApiError(payload.error);
    }
    throw new MainsAegisApiError({
      code: `http_${response.status}`,
      message: describeHttpFailure(response, path, responseText),
      retryable: response.status >= 500,
      details: {
        path,
        status: response.status,
        statusText: response.statusText,
        responseText: responseText.slice(0, 512),
      },
    });
  }

  return payload as T;
}

function bridgeAuthHeaders(baseUrl: string): Record<string, string> {
  const token = bridgeAuthToken(baseUrl);
  return token ? { [APP_SESSION_HEADER]: token } : {};
}

export function bridgeAuthToken(baseUrl: string): string | null {
  if (
    typeof window === "undefined" ||
    isMockBaseUrl(baseUrl) ||
    normalizeBaseUrl(baseUrl) !== ""
  )
    return null;
  const token =
    document
      .querySelector<HTMLMetaElement>(APP_SESSION_META)
      ?.content?.trim() ?? "";
  return token && !token.startsWith(RUNTIME_PLACEHOLDER_PREFIX) ? token : null;
}

export function httpServiceMode(): string | null {
  if (typeof document === "undefined") return null;
  const mode =
    document
      .querySelector<HTMLMetaElement>(HTTP_SERVICE_MODE_META)
      ?.content?.trim() ?? "";
  return mode && !mode.startsWith(RUNTIME_PLACEHOLDER_PREFIX) ? mode : null;
}

export function isHostedHttpServiceApp(): boolean {
  return httpServiceMode() === "hosted" && bridgeAuthToken("") !== null;
}

export async function getBridgeBootstrap(
  baseUrl: string,
): Promise<BridgeBootstrap | null> {
  if (isMockBaseUrl(baseUrl)) return null;
  try {
    return await requestJson<BridgeBootstrap>(baseUrl, "/api/v1/bootstrap");
  } catch {
    return null;
  }
}

export async function bridgeAuthRequired(baseUrl: string): Promise<boolean> {
  return (
    bridgeAuthToken(baseUrl) !== null ||
    (await getBridgeBootstrap(baseUrl))?.token_required === true
  );
}

function parseJsonPayload<T>(text: string): T | ApiErrorEnvelope | null {
  if (!text) return null;
  try {
    return JSON.parse(text) as T | ApiErrorEnvelope;
  } catch {
    return null;
  }
}

function describeHttpFailure(
  response: Response,
  path: string,
  responseText: string,
): string {
  if (path === "/api/v1/defmt/decode" && response.status >= 500) {
    return "defmt decode API is unavailable. Check that the Web dev server proxies /api to the running mains-aegis-devd instance.";
  }
  const statusText = response.statusText || "request failed";
  const body = responseText.trim();
  return body ? `${statusText}: ${body.slice(0, 160)}` : statusText;
}

function requestMock<T>(
  baseUrl: string,
  path: string,
  method: "GET" | "POST" | "DELETE" = "GET",
  body?: unknown,
): Promise<T> {
  if (
    baseUrl === "mock:usb" ||
    baseUrl === "mock:devd" ||
    baseUrl === "mock:devd-multi" ||
    baseUrl === "mock:devd-bind-target"
  ) {
    return requestMockDevd<T>(baseUrl, path, method, body);
  }
  if (path === "/api/v1/ping" || path === "/health") {
    return Promise.resolve({ ok: true } as T);
  }
  if (path === "/api/v1/identity") {
    return Promise.resolve(getMockIdentity(baseUrl) as T);
  }
  if (path === "/api/v1/network") {
    return Promise.resolve(getMockNetwork(baseUrl) as T);
  }
  if (path === "/api/v1/status") {
    return Promise.resolve(getMockStatus(baseUrl) as T);
  }
  if (path === "/api/v1/settings") {
    return Promise.resolve(mockSettingsForBaseUrl(baseUrl) as T);
  }
  if (path === "/api/v1/settings/advanced-power" && method === "POST") {
    updateMockAdvancedPower(baseUrl, body);
    return Promise.resolve({ advanced_power: "updated" } as T);
  }
  if (path === "/api/v1/settings/advanced-power/reset" && method === "POST") {
    resetMockAdvancedPower(baseUrl);
    return Promise.resolve({ advanced_power: "reset" } as T);
  }
  throw new MainsAegisApiError({
    code: "not_found",
    message: "mock endpoint not found",
    retryable: false,
    details: { path },
  });
}

function requestMockDevd<T>(
  baseUrl: string,
  path: string,
  method: "GET" | "POST" | "DELETE" = "GET",
  body?: unknown,
): Promise<T> {
  const bindTargetMock = baseUrl === "mock:devd-bind-target";
  const bindTargetState = bindTargetMock
    ? (mockBindTargetStateByBaseUrl.get(baseUrl) ?? {
        boundLogicalDeviceId: null,
        dismissedCompanion: false,
      })
    : null;
  if (bindTargetMock && bindTargetState) {
    mockBindTargetStateByBaseUrl.set(baseUrl, bindTargetState);
  }
  const hostedDiscoveryMock =
    baseUrl === "mock:devd" || baseUrl === "mock:devd-multi" || bindTargetMock;
  const multiChannelMock = baseUrl === "mock:devd-multi";
  const devdIdentity = hostedDiscoveryMock
    ? {
        ...getMockIdentity("mock:lab-standby"),
        device_id: "mains-aegis-devd-service",
        hostname: "mains-aegis-devd-service",
        hostname_fqdn: "mains-aegis-devd-service.local",
        short_id: "devd01",
        network: {
          ...getMockIdentity("mock:lab-standby").network,
          device_id: "mains-aegis-devd-service",
          hostname: "mains-aegis-devd-service",
          hostname_fqdn: "mains-aegis-devd-service.local",
        },
      }
    : null;
  const identity = multiChannelMock
    ? getMockIdentity("mock:lab-standby")
    : (devdIdentity ?? getMockIdentity("mock:usb"));
  const network = identity.network;
  const status = getMockStatus(
    hostedDiscoveryMock ? "mock:lab-standby" : "mock:usb",
  );
  const usbIdentity = bindTargetMock
    ? bindTargetState?.boundLogicalDeviceId
      ? getMockIdentity("mock:lab-standby")
      : null
    : multiChannelMock
      ? getMockIdentity("mock:lab-standby")
      : (devdIdentity ?? getMockIdentity("mock:usb"));
  const usbDevice = {
    id: hostedDiscoveryMock
      ? bindTargetMock
        ? "mock-devd-usb-pending"
        : multiChannelMock
          ? "mock-devd-usb-standby"
          : "mains-aegis-devd-service"
      : "mock-devd-esp32s3-1",
    display_name: hostedDiscoveryMock
      ? bindTargetMock
        ? "Pending USB CDC"
        : multiChannelMock
          ? "USB standby UPS"
          : "Bound ESP32-S3"
      : "USB demo CDC",
    port_path: "/dev/tty.usbmodem-demo",
    transport: bindTargetMock ? ("native_serial" as const) : ("mock" as const),
    binding:
      bindTargetMock && bindTargetState?.boundLogicalDeviceId
        ? {
            alias: "USB demo CDC",
            stable_id: "mock-devd-usb-pending",
            port_path: "/dev/tty.usbmodem-demo",
            created_at: "2026-04-28T00:00:00.000Z",
            logical_device_id: bindTargetState.boundLogicalDeviceId,
          }
        : hostedDiscoveryMock && !bindTargetMock
        ? {
            alias: "USB demo CDC",
            stable_id: multiChannelMock
              ? "mock-devd-usb-standby"
              : "mains-aegis-devd-service",
            port_path: "/dev/tty.usbmodem-demo",
            created_at: "2026-04-28T00:00:00.000Z",
            logical_device_id: multiChannelMock ? "mains-aegis-a1b2c3" : null,
          }
        : null,
    connection: "connected" as const,
    identity: usbIdentity,
    companion_lan_candidate:
      bindTargetMock &&
      bindTargetState?.boundLogicalDeviceId &&
      !bindTargetState.dismissedCompanion
        ? {
            mdns_host: "mains-aegis-a1b2c3.local",
            ip: "192.168.31.42",
            port: 80,
            detected_at: "2026-06-08T00:00:00.000Z",
            verified_at: "2026-06-08T00:00:00.000Z",
            source: "usb_bind_probe",
          }
        : hostedDiscoveryMock && !bindTargetMock
        ? {
            mdns_host: "mains-aegis-a1b2c3.local",
            ip: "192.168.31.42",
            port: 80,
            detected_at: "2026-06-08T00:00:00.000Z",
            verified_at: "2026-06-08T00:00:00.000Z",
            source: "usb_bind_probe",
          }
        : null,
    selected_artifact_id: hostedDiscoveryMock
      ? "mains-aegis-esp32s3-release-web_serial-c805b6a"
      : null,
    log_decode: {
      status:
        hostedDiscoveryMock && !bindTargetMock ? "verified" : "unverified",
      reason:
        hostedDiscoveryMock && !bindTargetMock
          ? null
          : "Device is not bound yet.",
      artifact_id: hostedDiscoveryMock
        ? "mains-aegis-esp32s3-release-web_serial-c805b6a"
        : null,
    },
  };
  const lanDevice = hostedDiscoveryMock
    ? {
        id: "mock-devd-lan-standby",
        display_name: "LAN standby UPS",
        port_path: null,
        lan_address: "mock:lab-standby",
        lan_conflict_addresses: [],
        transport: "lan" as const,
        binding: null,
        connection: "connected" as const,
        identity: getMockIdentity("mock:lab-standby"),
        status: getMockStatus("mock:lab-standby"),
        selected_artifact_id: null,
        log_decode: {
          status: "unverified",
          reason: null,
          artifact_id: null,
        },
      }
    : null;
  const devices = lanDevice ? [usbDevice, lanDevice] : [usbDevice];

  if (path === "/api/v1/ping" || path === "/health")
    return Promise.resolve({ ok: true } as T);
  if (path === "/api/v1/identity") return Promise.resolve(identity as T);
  if (path === "/api/v1/network") return Promise.resolve(network as T);
  if (path === "/api/v1/status") return Promise.resolve(status as T);
  if (path === "/api/v1/settings")
    return Promise.resolve(mockSettingsForBaseUrl(baseUrl) as T);
  if (path.match(/^\/api\/v1\/devices\/[^/]+\/settings$/)) {
    return Promise.resolve(mockSettingsForBaseUrl(baseUrl) as T);
  }
  if (path === "/api/v1/serial/session") {
    return Promise.resolve({
      connected: true,
      protocol: "mains-aegis.cdc.v1",
      status,
      logs: [],
      trace: [],
      settings: mockSettingsForBaseUrl(baseUrl),
    } as T);
  }
  if (path.match(/^\/api\/v1\/devices\/[^/]+\/trace(\?.*)?$/)) {
    return Promise.resolve({
      connected: true,
      protocol: "mains-aegis.cdc.v1",
      status,
      logs: [],
      trace: [],
      settings: mockSettingsForBaseUrl(baseUrl),
    } as T);
  }
  if (path === "/api/v1/settings/advanced-power" && method === "POST") {
    updateMockAdvancedPower(baseUrl, body);
    return Promise.resolve({ advanced_power: "updated" } as T);
  }
  if (
    path.startsWith("/api/v1/settings/advanced-power/reset") &&
    method === "POST"
  ) {
    resetMockAdvancedPower(baseUrl);
    return Promise.resolve({ advanced_power: "reset" } as T);
  }
  if (path === "/api/v1/devices") return Promise.resolve({ devices } as T);
  if (path === "/api/v1/devices/scan") return Promise.resolve({ devices } as T);
  if (path.endsWith("/bind")) {
    if (bindTargetMock && bindTargetState) {
      bindTargetState.boundLogicalDeviceId = "mains-aegis-a1b2c3";
      bindTargetState.dismissedCompanion = false;
      mockBindTargetStateByBaseUrl.set(baseUrl, bindTargetState);
    }
    return Promise.resolve({
      ...usbDevice,
      binding: {
        alias: "USB demo CDC",
        stable_id: usbDevice.id,
        port_path: usbDevice.port_path,
        created_at: "2026-04-28T00:00:00.000Z",
        logical_device_id:
          multiChannelMock || bindTargetMock ? "mains-aegis-a1b2c3" : null,
      },
      log_decode: { status: "unverified", reason: null, artifact_id: null },
    } as T);
  }
  if (path.endsWith("/companion-lan") && method === "POST") {
    const mockLanBaseUrl = bindTargetMock ? "mock:lab-standby" : null;
    return Promise.resolve({
      ...usbDevice,
      binding: {
        alias: "USB demo CDC",
        stable_id: usbDevice.id,
        port_path: usbDevice.port_path,
        created_at: "2026-04-28T00:00:00.000Z",
        logical_device_id:
          multiChannelMock || bindTargetMock ? "mains-aegis-a1b2c3" : null,
        lan_companion: {
          mdns_host: mockLanBaseUrl ?? "mains-aegis-a1b2c3.local",
          ip: mockLanBaseUrl ?? "192.168.31.42",
          port: 80,
          confirmed_at: "2026-06-08T00:00:00.000Z",
          last_verified_at: "2026-06-08T00:00:00.000Z",
        },
      },
      companion_lan_candidate: null,
      lan_address: mockLanBaseUrl ?? "192.168.31.42",
      lan_conflict_addresses: [],
      log_decode: { status: "unverified", reason: null, artifact_id: null },
    } as T);
  }
  if (path.endsWith("/companion-lan") && method === "DELETE") {
    if (bindTargetMock && bindTargetState) {
      bindTargetState.dismissedCompanion = true;
      mockBindTargetStateByBaseUrl.set(baseUrl, bindTargetState);
    }
    return Promise.resolve({
      ...usbDevice,
      binding: {
        alias: "USB demo CDC",
        stable_id: usbDevice.id,
        port_path: usbDevice.port_path,
        created_at: "2026-04-28T00:00:00.000Z",
        logical_device_id:
          multiChannelMock || bindTargetMock ? "mains-aegis-a1b2c3" : null,
      },
      companion_lan_candidate: null,
      lan_address: null,
      lan_conflict_addresses: [],
      log_decode: { status: "unverified", reason: null, artifact_id: null },
    } as T);
  }
  if (path.endsWith("/artifact")) {
    return Promise.resolve({
      ok: true,
      artifact: null,
    } as T);
  }
  if (path.endsWith("/flash")) {
    const dryRun = path.includes("/flash") ? true : false;
    return Promise.resolve({
      ok: true,
      dry_run: dryRun,
    } as T);
  }
  if (path.endsWith("/disconnect"))
    return Promise.resolve({ ...usbDevice, connection: "disconnected" } as T);
  if (path.endsWith("/connect"))
    return Promise.resolve({ ...usbDevice, connection: "connected" } as T);
  throw new MainsAegisApiError({
    code: "not_found",
    message: "mock devd endpoint not found",
    retryable: false,
    details: { path, baseUrl },
  });
}

export const ping = (baseUrl: string, options?: RequestOptions) =>
  requestJson<{ ok: true }>(baseUrl, "/api/v1/ping", options);
function leaseQuery(leaseId?: string) {
  return leaseId ? `?lease_id=${encodeURIComponent(leaseId)}` : "";
}

export const getIdentity = (
  baseUrl: string,
  leaseId?: string,
  options?: RequestOptions,
) =>
  requestJson<Identity>(
    baseUrl,
    `/api/v1/identity${leaseQuery(leaseId)}`,
    options,
  );
export const getNetwork = (
  baseUrl: string,
  leaseId?: string,
  options?: RequestOptions,
) =>
  requestJson<NetworkSummary>(
    baseUrl,
    `/api/v1/network${leaseQuery(leaseId)}`,
    options,
  );
export const getStatus = (
  baseUrl: string,
  leaseId?: string,
  options?: RequestOptions,
) =>
  requestJson<UpsStatus>(
    baseUrl,
    `/api/v1/status${leaseQuery(leaseId)}`,
    options,
  );
export const getSettings = (
  baseUrl: string,
  leaseId?: string,
  options?: RequestOptions,
) =>
  requestJson<DeviceSettings>(
    baseUrl,
    `/api/v1/settings${leaseQuery(leaseId)}`,
    options,
  );

export type DevdSerialSession = {
  connected: boolean;
  protocol: string;
  status?: UpsStatus | null;
  logs: SerialLogEntry[];
  trace: SerialTraceEntry[];
  settings: DeviceSettings;
};

type RawDevdSerialSession = {
  connected: boolean;
  protocol: string;
  identity?: Identity | null;
  status?: UpsStatus | null;
  logs: SerialLogEntry[];
  trace: SerialTraceEntry[];
  settings?: DeviceSettings;
};

export type DevdSerialEvent = {
  id: string;
  timestamp: string;
  device_id: string | null;
  kind: "serial_trace" | "serial_log" | "monitor" | string;
  message: string;
  payload: {
    trace?: SerialTraceEntry;
    log?: SerialLogEntry;
    status?: UpsStatus;
    [key: string]: unknown;
  };
};

export type DevdSerialEventStream = {
  close: () => void;
};

export type DevdDeviceEvent = {
  id: string;
  timestamp: string;
  device_id: string | null;
  kind: string;
  message: string;
  payload: unknown;
};

type DevdSerialSessionOptions = {
  logsLimit?: number;
  traceLimit?: number;
  leaseId?: string;
};

function devdSerialSessionPath(options: DevdSerialSessionOptions = {}) {
  const params = new URLSearchParams();
  if (options.logsLimit !== undefined)
    params.set("logs_limit", String(options.logsLimit));
  if (options.traceLimit !== undefined)
    params.set("trace_limit", String(options.traceLimit));
  if (options.leaseId) params.set("lease_id", options.leaseId);
  const query = params.toString();
  return `/api/v1/serial/session${query ? `?${query}` : ""}`;
}

export const getDevdSerialSession = (
  baseUrl: string,
  options?: DevdSerialSessionOptions,
) =>
  requestJson<RawDevdSerialSession>(baseUrl, devdSerialSessionPath(options), {
    bridgeAuth: true,
  }).then((session) => ({
    connected: session.connected,
    protocol: session.protocol,
    status: session.status,
    logs: session.logs,
    trace: session.trace,
    settings: session.settings ?? defaultMockSettings(),
  }));

export const createDevdWebLease = (baseUrl: string, deviceId: string) =>
  requestWithBody<DevdWebLease>(
    baseUrl,
    "/api/v1/serial/lease",
    "POST",
    { device_id: deviceId },
    { bridgeAuth: true },
  );
export const heartbeatDevdWebLease = (baseUrl: string, leaseId: string) =>
  requestWithBody<Omit<DevdWebLease, "device">>(
    baseUrl,
    `/api/v1/serial/lease/${encodeURIComponent(leaseId)}`,
    "POST",
    undefined,
    { bridgeAuth: true },
  );
export const releaseDevdWebLease = (
  baseUrl: string,
  leaseId: string,
  keepalive = false,
) => {
  const path = `/api/v1/serial/lease/${encodeURIComponent(leaseId)}`;
  if (keepalive && !isMockBaseUrl(baseUrl)) {
    return fetch(`${baseUrl}${path}`, {
      method: "DELETE",
      keepalive,
      headers: { Accept: "application/json", ...bridgeAuthHeaders(baseUrl) },
    }).then(() => undefined);
  }
  return requestWithBody<unknown>(baseUrl, path, "DELETE", undefined, {
    bridgeAuth: true,
  });
};

export function subscribeDevdSerialEvents(
  baseUrl: string,
  leaseId: string,
  callbacks: {
    onEvent: (event: DevdSerialEvent) => void;
    onError: (event: Event) => void;
  },
): DevdSerialEventStream {
  const params = new URLSearchParams({ lease_id: leaseId });
  const token = bridgeAuthToken(baseUrl);
  if (token) params.set(APP_SESSION_QUERY_PARAM, token);
  const eventSource = new EventSource(
    `${baseUrl}/api/v1/serial/events?${params.toString()}`,
  );
  const handleEvent = (event: Event) => {
    callbacks.onEvent(
      JSON.parse((event as MessageEvent<string>).data) as DevdSerialEvent,
    );
  };
  eventSource.addEventListener("serial_trace", handleEvent);
  eventSource.addEventListener("serial_log", handleEvent);
  eventSource.addEventListener("serial_status", handleEvent);
  eventSource.addEventListener("monitor", handleEvent);
  eventSource.onerror = callbacks.onError;
  return { close: () => eventSource.close() };
}

export function subscribeDevdDeviceEvents(
  baseUrl: string,
  callbacks: {
    onEvent: (event: DevdDeviceEvent) => void;
    onError: (event: Event) => void;
  },
): DevdSerialEventStream {
  if (isMockBaseUrl(baseUrl)) return { close: () => undefined };

  const params = new URLSearchParams();
  const token = bridgeAuthToken(baseUrl);
  if (token) params.set(APP_SESSION_QUERY_PARAM, token);
  const query = params.toString();
  const eventSource = new EventSource(
    `${baseUrl}/api/v1/devices/events${query ? `?${query}` : ""}`,
  );
  const handleEvent = (event: Event) => {
    callbacks.onEvent(
      JSON.parse((event as MessageEvent<string>).data) as DevdDeviceEvent,
    );
  };
  for (const kind of [
    "scan",
    "bind",
    "unbind",
    "connect",
    "disconnect",
    "artifact",
    "flash",
    "reset",
    "power_diag",
  ]) {
    eventSource.addEventListener(kind, handleEvent);
  }
  eventSource.onerror = callbacks.onError;
  return { close: () => eventSource.close() };
}

export type DevdWifiConfigApplyResult = {
  wifi_config: unknown;
  network: WifiApplyNetwork;
  applied: true;
};

export const sendDeviceWifiConfig = (
  baseUrl: string,
  input: { ssid: string; psk: string },
) => requestWithBody<unknown>(baseUrl, "/api/v1/wifi-config", "POST", input);
export const clearDeviceWifiConfig = (baseUrl: string) =>
  requestWithBody<unknown>(baseUrl, "/api/v1/wifi-config", "DELETE", undefined);
export const setDeviceLogLevel = (
  baseUrl: string,
  level: DeviceSettings["log_level"],
) =>
  requestWithBody<unknown>(baseUrl, "/api/v1/settings/log-level", "POST", {
    level,
  });
export const setDeviceManualChargePrefs = (
  baseUrl: string,
  prefs: DeviceSettings["manual_charge"],
) =>
  requestWithBody<unknown>(
    baseUrl,
    "/api/v1/settings/manual-charge",
    "POST",
    prefs,
  );
export const setDeviceAdvancedPower = (
  baseUrl: string,
  advancedPower: AdvancedPowerSettings,
) =>
  requestWithBody<unknown>(
    baseUrl,
    "/api/v1/settings/advanced-power",
    "POST",
    advancedPower,
  );
export const resetDeviceAdvancedPower = (baseUrl: string) =>
  requestWithBody<unknown>(
    baseUrl,
    "/api/v1/settings/advanced-power/reset",
    "POST",
    {},
  );
export const sendDevdWifiConfig = (
  baseUrl: string,
  deviceId: string,
  leaseId: string | null,
  input: { ssid: string; psk: string },
) =>
  requestWithBody<DevdWifiConfigApplyResult>(
    baseUrl,
    "/api/v1/wifi-config",
    "POST",
    { ...input, device_id: deviceId, lease_id: leaseId ?? undefined },
    { bridgeAuth: true },
  );
export const clearDevdWifiConfig = (
  baseUrl: string,
  deviceId: string,
  leaseId: string | null,
) =>
  requestWithBody<DevdWifiConfigApplyResult>(
    baseUrl,
    `/api/v1/wifi-config?${settingsTargetQuery(deviceId, leaseId)}`,
    "DELETE",
    undefined,
    { bridgeAuth: true },
  );
export const setDevdLogLevel = (
  baseUrl: string,
  deviceId: string,
  leaseId: string | null,
  level: DeviceSettings["log_level"],
) =>
  requestWithBody<unknown>(
    baseUrl,
    "/api/v1/settings/log-level",
    "POST",
    { level, device_id: deviceId, lease_id: leaseId ?? undefined },
    { bridgeAuth: true },
  );
export const setDevdManualChargePrefs = (
  baseUrl: string,
  deviceId: string,
  leaseId: string | null,
  prefs: DeviceSettings["manual_charge"],
) =>
  requestWithBody<unknown>(
    baseUrl,
    "/api/v1/settings/manual-charge",
    "POST",
    { ...prefs, device_id: deviceId, lease_id: leaseId ?? undefined },
    { bridgeAuth: true },
  );
export const setDevdAdvancedPower = (
  baseUrl: string,
  deviceId: string,
  leaseId: string | null,
  advancedPower: AdvancedPowerSettings,
) =>
  requestWithBody<unknown>(
    baseUrl,
    "/api/v1/settings/advanced-power",
    "POST",
    {
      ...advancedPower,
      device_id: deviceId,
      lease_id: leaseId ?? undefined,
    },
    { bridgeAuth: true },
  );
export const resetDevdAdvancedPower = (
  baseUrl: string,
  deviceId: string,
  leaseId: string | null,
) =>
  requestWithBody<unknown>(
    baseUrl,
    `/api/v1/settings/advanced-power/reset?${settingsTargetQuery(deviceId, leaseId)}`,
    "POST",
    undefined,
    { bridgeAuth: true },
  );

function settingsTargetQuery(deviceId: string, leaseId: string | null) {
  const query = new URLSearchParams({ device_id: deviceId });
  if (leaseId) query.set("lease_id", leaseId);
  return query.toString();
}

export async function probeDevice(
  baseUrl: string,
  leaseId?: string,
  options?: RequestOptions,
): Promise<ProbeResult> {
  await ping(baseUrl, options);
  const identity = await getIdentity(baseUrl, leaseId, options);
  const network = await getNetwork(baseUrl, leaseId, options);
  const status = await getStatus(baseUrl, leaseId, options);
  const settings = await getSettings(baseUrl, leaseId, options);
  return { identity, network, status, settings };
}

export function toErrorEnvelope(error: unknown): ApiErrorEnvelope["error"] {
  if (error instanceof MainsAegisApiError) return error.envelope;
  if (error instanceof Error) {
    return {
      code: "transport_error",
      message: error.message,
      retryable: true,
      details: null,
    };
  }
  return {
    code: "unknown_error",
    message: "unknown request failure",
    retryable: true,
    details: null,
  };
}

export const loadBundledFirmwareCatalog = () =>
  requestJson<FirmwareCatalog>("", "/firmware/firmware-catalog.json");
export const loadFirmwareCatalogFromUrl = (url: string) =>
  requestJson<FirmwareCatalog>("", url);
export const decodeDefmtFrame = (
  input: { elf_path: string; frame_hex: string },
  baseUrl = "",
) =>
  requestWithBody<DefmtDecodeResult>(
    baseUrl,
    "/api/v1/defmt/decode",
    "POST",
    input,
    { bridgeAuth: true },
  );
export const listDevdDevices = (baseUrl = "") =>
  requestJson<{ devices: DevdDevice[] }>(baseUrl, "/api/v1/devices", {
    bridgeAuth: true,
  });
export const scanDevdDevices = (baseUrl = "") =>
  requestWithBody<{ devices: DevdDevice[]; scan_trace?: DevdScanTraceEntry[] }>(
    baseUrl,
    "/api/v1/devices/scan",
    "POST",
    undefined,
    { bridgeAuth: true },
  );
export const getDevdDeviceIdentity = (baseUrl: string, deviceId: string) =>
  requestJson<Identity>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/identity`,
    { bridgeAuth: true },
  );
export const getDevdDeviceSettings = (baseUrl: string, deviceId: string) =>
  requestJson<DeviceSettings>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/settings`,
    { bridgeAuth: true },
  );
export const getDevdDeviceTrace = (
  baseUrl: string,
  deviceId: string,
  options?: DevdSerialSessionOptions,
) =>
  requestJson<RawDevdSerialSession>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/trace${devdSerialSessionPath(options).replace("/api/v1/serial/session", "")}`,
    { bridgeAuth: true },
  );
export const bindDevdDevice = (
  deviceId: string,
  input: { alias?: string; logicalDeviceId?: string } = {},
  baseUrl = "",
) =>
  requestWithBody<DevdDevice>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/bind`,
    "POST",
    {
      alias: input.alias,
      logical_device_id: input.logicalDeviceId,
    },
    { bridgeAuth: true },
  );
export const bindDevdCompanionLan = (
  deviceId: string,
  input: { mdns_host?: string; ip?: string; port?: number } = {},
  baseUrl = "",
) =>
  requestWithBody<DevdDevice>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/companion-lan`,
    "POST",
    input,
    { bridgeAuth: true },
  );
export const clearDevdCompanionLan = (deviceId: string, baseUrl = "") =>
  requestWithBody<DevdDevice>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/companion-lan`,
    "DELETE",
    undefined,
    { bridgeAuth: true },
  );
export const connectDevdDevice = (deviceId: string, baseUrl = "") =>
  requestWithBody<DevdDevice>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/connect`,
    "POST",
    undefined,
    { bridgeAuth: true },
  );
export const disconnectDevdDevice = (deviceId: string, baseUrl = "") =>
  requestWithBody<DevdDevice>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/disconnect`,
    "POST",
    undefined,
    { bridgeAuth: true },
  );
export const selectDevdArtifact = (
  deviceId: string,
  input: {
    artifact_id?: string;
    manifest_path?: string;
    artifact?: FirmwareCatalog["artifacts"][number];
  },
  baseUrl = "",
) =>
  requestWithBody<unknown>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/artifact`,
    "POST",
    input,
    { bridgeAuth: true },
  );
export const flashDevdDevice = (
  deviceId: string,
  input: { artifact_id?: string; dry_run?: boolean },
  baseUrl = "",
) =>
  requestWithBody<unknown>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/flash`,
    "POST",
    input,
    { bridgeAuth: true },
  );

function defaultMockSettings(ratedVoutMv = 12_000): DeviceSettings {
  return {
    wifi: {
      configured: false,
      ssid: null,
    },
    log_level: "info",
    manual_charge: {
      target: "full_100",
      speed: "ma_500",
      timer_h: 2,
    },
    advanced_power: {
      standby_drop_mv: 1200,
      assist_low_drop_mv: 600,
      assist_enter_delta_ma: 0,
      assist_exit_delta_ma: 0,
      assist_required_samples: 2,
      assist_ramp_step_mv: 100,
      assist_ramp_interval_ms: 200,
      rated_enter_delta_ma: 0,
      rated_exit_delta_ma: 0,
      vin_drop_threshold_pct: 4,
      required_samples: 2,
    },
    advanced_power_capabilities: {
      rated_vout_mv: ratedVoutMv,
      standby_drop_mv: { default: 1200, min: 0, max: 3000, step: 20 },
      assist_low_drop_mv: { default: 600, min: 0, max: 3000, step: 20 },
      assist_enter_delta_ma: { default: 0, min: -100, max: 1000, step: 50 },
      assist_exit_delta_ma: { default: 0, min: -50, max: 1000, step: 50 },
      assist_required_samples: { default: 2, min: 1, max: 5, step: 1 },
      assist_ramp_step_mv: { default: 100, min: 20, max: 1000, step: 20 },
      assist_ramp_interval_ms: { default: 200, min: 100, max: 3000, step: 100 },
      rated_enter_delta_ma: { default: 0, min: -100, max: 1000, step: 50 },
      rated_exit_delta_ma: { default: 0, min: -50, max: 1000, step: 50 },
      vin_drop_threshold_pct: { default: 4, min: 1, max: 12, step: 1 },
      required_samples: { default: 2, min: 1, max: 5, step: 1 },
    },
  };
}

function mockRatedVoutMv(baseUrl: string): number {
  return getMockIdentity(baseUrl).hardware_capabilities?.rated_vout_mv ?? 12_000;
}

function mockSettingsForBaseUrl(baseUrl: string): DeviceSettings {
  if (!mockSettingsByBaseUrl.has(baseUrl)) {
    mockSettingsByBaseUrl.set(baseUrl, defaultMockSettings(mockRatedVoutMv(baseUrl)));
  }
  return mockSettingsByBaseUrl.get(baseUrl)!;
}

function updateMockAdvancedPower(baseUrl: string, body: unknown) {
  if (!body || typeof body !== "object") return;
  const current = mockSettingsForBaseUrl(baseUrl);
  const next = body as Partial<AdvancedPowerSettings>;
  mockSettingsByBaseUrl.set(baseUrl, {
    ...current,
    advanced_power: {
      standby_drop_mv:
        typeof next.standby_drop_mv === "number"
          ? next.standby_drop_mv
          : current.advanced_power.standby_drop_mv,
      assist_low_drop_mv:
        typeof next.assist_low_drop_mv === "number"
          ? next.assist_low_drop_mv
          : current.advanced_power.assist_low_drop_mv,
      assist_enter_delta_ma:
        typeof next.assist_enter_delta_ma === "number"
          ? next.assist_enter_delta_ma
          : current.advanced_power.assist_enter_delta_ma,
      assist_exit_delta_ma:
        typeof next.assist_exit_delta_ma === "number"
          ? next.assist_exit_delta_ma
          : current.advanced_power.assist_exit_delta_ma,
      assist_required_samples:
        typeof next.assist_required_samples === "number"
          ? next.assist_required_samples
          : current.advanced_power.assist_required_samples,
      assist_ramp_step_mv:
        typeof next.assist_ramp_step_mv === "number"
          ? next.assist_ramp_step_mv
          : current.advanced_power.assist_ramp_step_mv,
      assist_ramp_interval_ms:
        typeof next.assist_ramp_interval_ms === "number"
          ? next.assist_ramp_interval_ms
          : current.advanced_power.assist_ramp_interval_ms,
      rated_enter_delta_ma:
        typeof next.rated_enter_delta_ma === "number"
          ? next.rated_enter_delta_ma
          : current.advanced_power.rated_enter_delta_ma,
      rated_exit_delta_ma:
        typeof next.rated_exit_delta_ma === "number"
          ? next.rated_exit_delta_ma
          : current.advanced_power.rated_exit_delta_ma,
      vin_drop_threshold_pct:
        typeof next.vin_drop_threshold_pct === "number"
          ? next.vin_drop_threshold_pct
          : current.advanced_power.vin_drop_threshold_pct,
      required_samples:
        typeof next.required_samples === "number"
          ? next.required_samples
          : current.advanced_power.required_samples,
    },
  });
}

function resetMockAdvancedPower(baseUrl: string) {
  const current = mockSettingsForBaseUrl(baseUrl);
  const defaults = defaultMockSettings(current.advanced_power_capabilities.rated_vout_mv);
  mockSettingsByBaseUrl.set(baseUrl, {
    ...current,
    advanced_power: defaults.advanced_power,
  });
}
