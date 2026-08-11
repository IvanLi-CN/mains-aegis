import { getMockIdentity, getMockNetwork, getMockStatus } from "../fixtures/mockDevices";
import { isDemoQueryEnabled } from "../demo/query";
import {
  buildAdvancedPowerCapabilities,
  buildAdvancedPowerDefaults,
  resolvePreTpsVinMv,
} from "./runtimeModeProfiles";
import type {
  AdvancedPowerSettings,
  ActiveAlertsSnapshot,
  AppRuntimeMode,
  ApiErrorEnvelope,
  ChargeCapabilities,
  ChargeControlDetail,
  ChargeControlSummary,
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
  DevdDiagSnapshot,
  TpsEnableReleaseResponse,
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
const mockStatusByBaseUrl = new Map<string, UpsStatus>();
const mockAlertsByBaseUrl = new Map<string, ActiveAlertsSnapshot>();

export type ManualChargeControlRequest = {
  action: "start" | "stop";
  confirm_loop?: boolean;
};

export type ManualChargePreviewRequest = {
  target: DeviceSettings["manual_charge"]["target"];
  current_ma: number;
  timer_minutes: number;
  power_path: DeviceSettings["manual_charge"]["power_path"];
};

export type ManualChargeControlResponse = ChargeControlDetail;

export const isMockBaseUrl = (baseUrl: string) => baseUrl.startsWith("mock:");
const APP_SESSION_HEADER = "x-mains-aegis-app-session";
const APP_SESSION_QUERY_PARAM = "app_session";
const HTTP_SERVICE_MODE_META = 'meta[name="mains-aegis-http-service-mode"]';
const APP_RUNTIME_MODE_META = 'meta[name="mains-aegis-app-runtime-mode"]';
const APP_SESSION_META = 'meta[name="mains-aegis-app-session"]';
const RUNTIME_PLACEHOLDER_PREFIX = "__MAINS_AEGIS_";

type ChargeControlCompatSummary = NonNullable<UpsStatus["charge_control"]> & {
  binding_reason?: string | null;
  loop_confirmation_required?: boolean | null;
  loop_override_active?: boolean | null;
  remaining_minutes?: number | null;
  start_block_reason?: string | null;
};

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

function demoSeedEnabled(): boolean {
  if (typeof window === "undefined") return false;
  return isDemoQueryEnabled();
}

type RequestOptions = {
  bridgeAuth?: boolean;
  timeoutMs?: number;
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

  const demoMockBaseUrl =
    demoSeedEnabled() && canResolveDemoHttpMock(baseUrl)
      ? normalizeBaseUrl(baseUrl)
      : null;
  if (demoMockBaseUrl) {
    return requestMock<T>(demoMockBaseUrl, path, method, body);
  }

  const response = await fetch(`${baseUrl}${path}`, {
    signal: requestSignal(options.timeoutMs),
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
    if (
      response.status === 404 &&
      (path === "/api/v1/alerts" || path.startsWith("/api/v1/alerts/"))
    ) {
      throw new MainsAegisApiError({
        code: "unsupported",
        message: "Alerts are unavailable on this firmware",
        retryable: false,
        details: { ok: false, result: "unsupported" },
      });
    }
    if (payload && typeof payload === "object" && "error" in payload) {
      throw new MainsAegisApiError(payload.error);
    }
    if (
      payload &&
      typeof payload === "object" &&
      "result" in payload &&
      (payload.result === "stale" || payload.result === "inactive")
    ) {
      throw new MainsAegisApiError({
        code: payload.result,
        message:
          payload.result === "stale"
            ? "The alert instance is stale"
            : "The alert is no longer active",
        retryable: false,
        details: payload,
      });
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

function requestSignal(timeoutMs?: number): AbortSignal | undefined {
  if (!timeoutMs || timeoutMs <= 0 || typeof AbortSignal === "undefined") {
    return undefined;
  }
  if (typeof AbortSignal.timeout === "function") {
    return AbortSignal.timeout(timeoutMs);
  }
  if (typeof AbortController === "undefined") return undefined;
  const controller = new AbortController();
  setTimeout(() => controller.abort(), timeoutMs);
  return controller.signal;
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

export function appRuntimeMode(): AppRuntimeMode {
  if (typeof document === "undefined") return "unknown";
  const runtimeMode =
    document
      .querySelector<HTMLMetaElement>(APP_RUNTIME_MODE_META)
      ?.content?.trim() ?? "";
  if (runtimeMode && !runtimeMode.startsWith(RUNTIME_PLACEHOLDER_PREFIX)) {
    if (
      runtimeMode === "hosted" ||
      runtimeMode === "http_service_api_only" ||
      runtimeMode === "public_static"
    ) {
      return runtimeMode;
    }
  }
  if (httpServiceMode() === "hosted" && bridgeAuthToken("") !== null)
    return "hosted";
  if (httpServiceMode() === "api-only") return "http_service_api_only";
  return "unknown";
}

export function isHostedHttpServiceApp(): boolean {
  return httpServiceMode() === "hosted" && bridgeAuthToken("") !== null;
}

export function isPublicStaticApp(): boolean {
  return appRuntimeMode() === "public_static";
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
    return Promise.resolve(mockStatusForBaseUrl(baseUrl) as T);
  }
  if (path === "/api/v1/alerts") {
    return Promise.resolve(mockAlerts(baseUrl) as T);
  }
  if (path.match(/^\/api\/v1\/alerts\/[^/]+\/mute$/) && method === "POST") {
    return Promise.resolve(muteMockAlert(baseUrl, path, body) as T);
  }
  if (path === "/api/v1/charge-control") {
    return Promise.resolve(mockChargeControlDetailForBaseUrl(baseUrl) as T);
  }
  if (path === "/api/v1/settings") {
    return Promise.resolve(mockSettingsForBaseUrl(baseUrl) as T);
  }
  if (path === "/api/v1/charge-control/preview" && method === "POST") {
    return Promise.resolve(previewMockManualChargeControl(baseUrl, body) as T);
  }
  if (path === "/api/v1/settings/manual-charge" && method === "POST") {
    updateMockManualChargePrefs(baseUrl, body);
    return Promise.resolve({ manual_charge: "updated" } as T);
  }
  if (path === "/api/v1/control/manual-charge" && method === "POST") {
    return Promise.resolve(updateMockManualChargeControl(baseUrl, body) as T);
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

function canResolveDemoHttpMock(baseUrl: string): boolean {
  try {
    getMockIdentity(baseUrl);
    return true;
  } catch {
    return false;
  }
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
    port_path: "/tmp/fixture-usb-demo",
    transport: bindTargetMock ? ("native_serial" as const) : ("mock" as const),
    binding:
      bindTargetMock && bindTargetState?.boundLogicalDeviceId
        ? {
            alias: "USB demo CDC",
            stable_id: "mock-devd-usb-pending",
            port_path: "/tmp/fixture-usb-demo",
            created_at: "2026-04-28T00:00:00.000Z",
            logical_device_id: bindTargetState.boundLogicalDeviceId,
          }
        : hostedDiscoveryMock && !bindTargetMock
        ? {
            alias: "USB demo CDC",
            stable_id: multiChannelMock
              ? "mock-devd-usb-standby"
              : "mains-aegis-devd-service",
            port_path: "/tmp/fixture-usb-demo",
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
  if (path === "/api/v1/alerts") return Promise.resolve(mockAlerts(baseUrl) as T);
  if (path === "/api/v1/charge-control")
    return Promise.resolve(mockChargeControlDetailForBaseUrl(baseUrl) as T);
  if (path === "/api/v1/settings")
    return Promise.resolve(mockSettingsForBaseUrl(baseUrl) as T);
  if (path === "/api/v1/charge-control/preview" && method === "POST") {
    return Promise.resolve(previewMockManualChargeControl(baseUrl, body) as T);
  }
  if (path === "/api/v1/settings/manual-charge" && method === "POST") {
    updateMockManualChargePrefs(baseUrl, body);
    return Promise.resolve({ manual_charge: "updated" } as T);
  }
  if (path === "/api/v1/control/manual-charge" && method === "POST") {
    return Promise.resolve(updateMockManualChargeControl(baseUrl, body) as T);
  }
  if (path.match(/^\/api\/v1\/devices\/[^/]+\/charge-control$/)) {
    return Promise.resolve(mockChargeControlDetailForBaseUrl(baseUrl) as T);
  }
  if (
    path.match(/^\/api\/v1\/devices\/[^/]+\/charge-control\/preview$/) &&
    method === "POST"
  ) {
    return Promise.resolve(previewMockManualChargeControl(baseUrl, body) as T);
  }
  if (
    path.match(/^\/api\/v1\/devices\/[^/]+\/control\/manual-charge$/) &&
    method === "POST"
  ) {
    return Promise.resolve(updateMockManualChargeControl(baseUrl, body) as T);
  }
  if (path.match(/^\/api\/v1\/devices\/[^/]+\/settings$/)) {
    return Promise.resolve(mockSettingsForBaseUrl(baseUrl) as T);
  }
  if (path.match(/^\/api\/v1\/devices\/[^/]+\/alerts$/)) {
    return Promise.resolve(mockAlerts(baseUrl) as T);
  }
  if (
    path.match(/^\/api\/v1\/devices\/[^/]+\/alerts\/[^/]+\/mute$/) &&
    method === "POST"
  ) {
    return Promise.resolve(muteMockAlert(baseUrl, path, body) as T);
  }
  if (path.match(/^\/api\/v1\/devices\/[^/]+\/diag-snapshot(?:\?.*)?$/)) {
    return Promise.resolve({
      schema_version: 2,
      packages: {
        "mcu.runtime": {
          ok: true,
          source: "runtime_latch",
          captured_at_ms: 0,
          age_ms: 0,
          duration_ms: 0,
          payload: {
            tps_enable_interlock: {
              therm_kill_n_low: false,
              mcu_drive_low: false,
              tps_en_effective_inhibit: false,
              source: "released",
              asserted_at_ms: null,
              last_release_at_ms: null,
              failure_channel: null,
              failure_stage: null,
              failure_code: null,
            },
          },
        },
      },
      errors: {},
    } as T);
  }
  if (path === "/api/v1/serial/session") {
    return Promise.resolve({
      connected: true,
      protocol: "mains-aegis.cdc.v1",
      status: mockStatusForBaseUrl(baseUrl),
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
export async function getDeviceChargeControl(
  baseUrl: string,
  leaseId?: string,
  options?: RequestOptions,
): Promise<ChargeControlDetail> {
  try {
    const payload = await requestJson<unknown>(
      baseUrl,
      `/api/v1/charge-control${leaseQuery(leaseId)}`,
      options,
    );
    if (isChargeControlDetailPayload(payload)) return payload;
    const legacySummary = extractLegacyChargeControlSummary(payload);
    if (legacySummary) {
      return loadCompatibleDeviceChargeControl(
        baseUrl,
        leaseId,
        undefined,
        legacySummary,
        options,
      );
    }
    return loadCompatibleDeviceChargeControl(baseUrl, leaseId, undefined, undefined, options);
  } catch (error) {
    if (shouldFallbackToCompatibleChargeControl(error)) {
      return loadCompatibleDeviceChargeControl(baseUrl, leaseId, undefined, undefined, options);
    }
    throw error;
  }
}
export const previewDeviceChargeControl = (
  baseUrl: string,
  input: ManualChargePreviewRequest,
  leaseId?: string,
) =>
  previewDeviceChargeControlCompat(baseUrl, input, leaseId);

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

function isChargeControlDetailPayload(
  payload: unknown,
): payload is ChargeControlDetail {
  return Boolean(
    payload &&
      typeof payload === "object" &&
      "summary" in payload &&
      "readiness" in payload &&
      "telemetry" in payload &&
      "evidence" in payload,
  );
}

function extractLegacyChargeControlSummary(
  payload: unknown,
): Partial<ChargeControlCompatSummary> | undefined {
  if (!payload || typeof payload !== "object" || !("charge_control" in payload)) {
    return undefined;
  }
  const summary = (payload as { charge_control?: unknown }).charge_control;
  return summary && typeof summary === "object"
    ? (summary as Partial<ChargeControlCompatSummary>)
    : undefined;
}

function shouldFallbackToCompatibleChargeControl(error: unknown): boolean {
  return (
    error instanceof MainsAegisApiError &&
    (error.envelope.code === "not_found" || error.envelope.code === "http_404")
  );
}

function defaultCompatChargeControlSummary(): ChargeControlCompatSummary {
  return {
    mode: "auto",
    manual_active: false,
    takeover: false,
    stop_inhibit: false,
    last_stop_reason: null,
    requested_power_path: "auto",
    bound_power_path: null,
    start_state: "ready",
    output_power_w10: null,
    power_telemetry_fresh: true,
    binding_reason: null,
    loop_confirmation_required: false,
    loop_override_active: false,
    remaining_minutes: null,
    start_block_reason: null,
  };
}

function compatManualChargeCurrentMa(
  speed: DeviceSettings["manual_charge"]["speed"],
): number {
  if (speed === "ma_100") return 100;
  if (speed === "ma_1000") return 1000;
  return 500;
}

function compatManualChargePreviewFromSettings(
  settings: DeviceSettings,
): ManualChargePreviewRequest {
  return {
    target: settings.manual_charge.target,
    current_ma: compatManualChargeCurrentMa(settings.manual_charge.speed),
    timer_minutes: settings.manual_charge.timer_h * 60,
    power_path: settings.manual_charge.power_path ?? "auto",
  };
}

function compatChargeControlSummary(
  status: UpsStatus,
  summaryOverride?: Partial<ChargeControlCompatSummary>,
): ChargeControlCompatSummary {
  return {
    ...defaultCompatChargeControlSummary(),
    ...((status.charge_control ?? {}) as Partial<ChargeControlCompatSummary>),
    ...(summaryOverride ?? {}),
  };
}

function compatBoundPowerPath(
  status: UpsStatus,
  requestedPowerPath: string,
  summary: ChargeControlCompatSummary,
): string | null {
  if (requestedPowerPath === "dcin" || requestedPowerPath === "usbc") {
    return requestedPowerPath;
  }
  const directSource = status.input.source;
  if (directSource === "dcin" || directSource === "usbc") return directSource;
  return summary.bound_power_path ?? null;
}

function compatBindingReason(
  requestedPowerPath: string,
  boundPowerPath: string | null,
  summary: ChargeControlCompatSummary,
): string | null {
  if (summary.binding_reason) return summary.binding_reason;
  if (!boundPowerPath) return null;
  if (requestedPowerPath === "auto" && boundPowerPath === "dcin") {
    return "auto_dcin_fallback";
  }
  if (requestedPowerPath === "auto" && boundPowerPath === "usbc") {
    return "auto_usbc";
  }
  if (requestedPowerPath === "dcin") return "explicit_dcin";
  if (requestedPowerPath === "usbc") return "explicit_usbc";
  return null;
}

function compatChargeTelemetry(
  status: UpsStatus,
  settings: DeviceSettings,
  summary: ChargeControlCompatSummary,
) {
  const inputSource = status.input.source ?? "unknown";
  const dcinLimit = settings.charge_capabilities?.dcin_input_limit_ma ?? 1000;
  const maxOutput = settings.charge_capabilities?.max_output_current_ma ?? 3500;
  const pdMinVoltage =
    settings.charge_capabilities?.usb_pd_high_power_min_voltage_mv ?? 9000;
  const pdMaxVoltage =
    settings.charge_capabilities?.usb_pd_high_power_max_voltage_mv ?? 20000;
  const pdMinPowerMw =
    settings.charge_capabilities?.usb_pd_high_power_min_power_mw ?? 20000;
  return {
    input_source: inputSource,
    policy_target_ichg_ma: status.charger.policy_target_ichg_ma ?? null,
    ibat_actual_ma: status.charger.ibat_ma ?? null,
    target_voltage_mv:
      settings.charge_capabilities?.target_voltage_mv ?? 16800,
    iindpm_ma: status.charger.limit_active
      ? status.charger.limit_threshold_ma ?? null
      : null,
    vindpm_mv: resolvePreTpsVinMv(status.input) ?? null,
    output_power_w10: summary.output_power_w10 ?? null,
    power_telemetry_fresh: summary.power_telemetry_fresh ?? true,
    input_limit_summary:
      inputSource === "dcin"
        ? `DCIN <= ${dcinLimit} mA`
        : inputSource === "usbc"
          ? `USB-C PD gate ${pdMinVoltage / 1000}-${pdMaxVoltage / 1000} V / ${pdMinPowerMw / 1000} W`
          : null,
    output_limit_summary: `Max output ${maxOutput} mA`,
  };
}

function compatChargeControlDetail(
  status: UpsStatus,
  settings: DeviceSettings,
  previewRequest?: ManualChargePreviewRequest,
  summaryOverride?: Partial<ChargeControlCompatSummary>,
): ChargeControlDetail {
  const summary = compatChargeControlSummary(status, summaryOverride);
  const requestedPowerPath =
    previewRequest?.power_path ??
    summary.requested_power_path ??
    settings.manual_charge.power_path ??
    "auto";
  const boundPowerPath = compatBoundPowerPath(
    status,
    requestedPowerPath,
    summary,
  );
  const telemetry = compatChargeTelemetry(status, settings, summary);
  const loopGuardThreshold =
    settings.charge_capabilities?.loop_start_max_power_without_confirm_w10 ?? 20;
  const loopConfirmationRequired =
    boundPowerPath === "usbc" &&
    (!telemetry.power_telemetry_fresh ||
      telemetry.output_power_w10 === null ||
      telemetry.output_power_w10 >= loopGuardThreshold);
  const readinessState = summary.manual_active
    ? "running"
    : loopConfirmationRequired
      ? "confirm_required"
      : summary.start_state ?? "ready";
  const readinessAction = summary.manual_active
    ? "stop"
    : readinessState === "confirm_required"
      ? "confirm_loop"
      : readinessState === "blocked"
        ? "none"
        : "start";
  const startBlockCode =
    typeof summary.start_block_reason === "string" &&
    summary.start_block_reason.length > 0
      ? summary.start_block_reason
      : null;
  const remainingMinutes = summary.manual_active
    ? summary.remaining_minutes ??
      previewRequest?.timer_minutes ??
      settings.manual_charge.timer_h * 60
    : null;
  return {
    summary: {
      mode: summary.mode,
      manual_active: summary.manual_active,
      takeover: summary.takeover,
      stop_inhibit: summary.stop_inhibit,
      last_stop_reason: summary.last_stop_reason,
      remaining_minutes: remainingMinutes,
      loop_override_active: summary.loop_override_active ?? false,
    },
    readiness: {
      state: readinessState,
      action: readinessAction,
      planned_path: {
        requested: requestedPowerPath,
        bound: boundPowerPath,
        binding_reason: compatBindingReason(
          requestedPowerPath,
          boundPowerPath,
          summary,
        ),
      },
      block:
        readinessState === "blocked"
          ? {
              code: startBlockCode ?? "blocked_unknown",
              message: startBlockCode
                ? `Manual charge is blocked by ${startBlockCode}.`
                : "Manual charge is currently blocked.",
            }
          : null,
      loop_override: {
        required: loopConfirmationRequired,
        active: summary.loop_override_active ?? false,
        allowed_guards: [
          "loop_start_low_output_gate",
          "loop_telemetry_miss_latch",
          "loop_stop_high_output_latch",
        ],
      },
    },
    telemetry,
    evidence: [
      {
        source: "battery.charge_fet_on",
        code: "charge_fet_on",
        label: "Charge FET",
        value: status.battery.charge_fet_on ?? null,
      },
      {
        source: "charger.detail_status",
        code: "detail_status",
        label: "Charger status",
        value: status.charger.detail_status ?? null,
      },
      {
        source: "charger.vbat_present",
        code: "vbat_present",
        label: "Battery present",
        value: status.charger.vbat_present,
      },
      {
        source: "battery.no_battery",
        code: "no_battery",
        label: "No battery flag",
        value: status.battery.no_battery,
      },
      {
        source: "charge_control.output_power_w10",
        code: "output_power_w10",
        label: "Output power",
        value: telemetry.output_power_w10,
      },
      {
        source: "charge_control.power_telemetry_fresh",
        code: "power_telemetry_fresh",
        label: "Power telemetry fresh",
        value: telemetry.power_telemetry_fresh,
      },
    ],
  };
}

async function loadCompatibleDeviceChargeControl(
  baseUrl: string,
  leaseId?: string,
  previewRequest?: ManualChargePreviewRequest,
  summaryOverride?: Partial<ChargeControlCompatSummary>,
  options?: RequestOptions,
): Promise<ChargeControlDetail> {
  const [status, settings] = await Promise.all([
    getStatus(baseUrl, leaseId, options),
    getSettings(baseUrl, leaseId, options),
  ]);
  return compatChargeControlDetail(
    status,
    settings,
    previewRequest,
    summaryOverride,
  );
}

async function previewDeviceChargeControlCompat(
  baseUrl: string,
  input: ManualChargePreviewRequest,
  leaseId?: string,
): Promise<ChargeControlDetail> {
  try {
    const payload = await requestWithBody<unknown>(
      baseUrl,
      `/api/v1/charge-control/preview${leaseQuery(leaseId)}`,
      "POST",
      input,
    );
    if (isChargeControlDetailPayload(payload)) return payload;
    const legacySummary = extractLegacyChargeControlSummary(payload);
    if (legacySummary) {
      return loadCompatibleDeviceChargeControl(
        baseUrl,
        leaseId,
        input,
        legacySummary,
      );
    }
    return loadCompatibleDeviceChargeControl(baseUrl, leaseId, input);
  } catch (error) {
    if (shouldFallbackToCompatibleChargeControl(error)) {
      return loadCompatibleDeviceChargeControl(baseUrl, leaseId, input);
    }
    throw error;
  }
}

async function setDeviceManualChargeControlCompat(
  baseUrl: string,
  input: ManualChargeControlRequest,
): Promise<ChargeControlDetail> {
  const payload = await requestWithBody<unknown>(
    baseUrl,
    "/api/v1/control/manual-charge",
    "POST",
    input,
  );
  if (isChargeControlDetailPayload(payload)) return payload;
  const legacySummary = extractLegacyChargeControlSummary(payload);
  if (legacySummary) {
    return loadCompatibleDeviceChargeControl(
      baseUrl,
      undefined,
      undefined,
      legacySummary,
    );
  }
  return loadCompatibleDeviceChargeControl(baseUrl);
}

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
    "diag_snapshot",
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

export const getDeviceAlerts = (
  baseUrl: string,
  options?: RequestOptions,
) => requestJson<ActiveAlertsSnapshot>(baseUrl, "/api/v1/alerts", options);

export const muteDeviceAlert = (
  baseUrl: string,
  alertId: string,
  instanceId: number,
) =>
  requestWithBody<unknown>(
    baseUrl,
    `/api/v1/alerts/${encodeURIComponent(alertId)}/mute`,
    "POST",
    { instance_id: instanceId },
  );

export const getDevdDeviceAlerts = (
  baseUrl: string,
  deviceId: string,
  options?: RequestOptions,
) =>
  requestJson<ActiveAlertsSnapshot>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/alerts`,
    { bridgeAuth: true, ...options },
  );

export const muteDevdDeviceAlert = (
  baseUrl: string,
  deviceId: string,
  alertId: string,
  instanceId: number,
) =>
  requestWithBody<unknown>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/alerts/${encodeURIComponent(alertId)}/mute`,
    "POST",
    { instance_id: instanceId },
    { bridgeAuth: true },
  );

function mockAlerts(baseUrl: string): ActiveAlertsSnapshot {
  const current = mockAlertsByBaseUrl.get(baseUrl);
  if (current) return structuredClone(current);
  const initial: ActiveAlertsSnapshot = {
    alerts: [
      {
        alert_id: "mains_absent_dc",
        instance_id: 1,
        severity: "warning",
        sound_state: "audible",
      },
      {
        alert_id: "module_fault",
        instance_id: 2,
        severity: "critical",
        sound_state: "system_silent",
      },
    ],
  };
  mockAlertsByBaseUrl.set(baseUrl, initial);
  return structuredClone(initial);
}

function muteMockAlert(baseUrl: string, path: string, body: unknown): unknown {
  const alertId = decodeURIComponent(path.split("/").at(-2) ?? "");
  const instanceId = Number((body as { instance_id?: unknown } | undefined)?.instance_id);
  const snapshot = mockAlerts(baseUrl);
  const alert = snapshot.alerts.find((item) => item.alert_id === alertId);
  if (!alert || alert.instance_id !== instanceId) {
    throw new MainsAegisApiError({
      code: "stale_alert_instance",
      message: "The alert instance changed before it could be muted.",
      retryable: false,
      details: null,
    });
  }
  const result = alert.sound_state === "muted" ? "already_muted" : "muted";
  alert.sound_state = "muted";
  mockAlertsByBaseUrl.set(baseUrl, snapshot);
  return { ok: true, ...alert, result };
}
export const setDeviceManualChargeControl = (
  baseUrl: string,
  input: ManualChargeControlRequest,
) =>
  setDeviceManualChargeControlCompat(baseUrl, input);
export const getDevdDeviceChargeControl = (
  baseUrl: string,
  deviceId: string,
) =>
  requestJson<ChargeControlDetail>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/charge-control`,
    { bridgeAuth: true },
  );
export const getDevdDeviceDiagSnapshot = (baseUrl: string, deviceId: string) =>
  requestJson<DevdDiagSnapshot>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/diag-snapshot?package=mcu.runtime`,
    { bridgeAuth: true },
  );
export const releaseDevdTpsEnableInterlock = (
  baseUrl: string,
  deviceId: string,
  leaseId: string,
) =>
  requestWithBody<TpsEnableReleaseResponse>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/tps-en/release`,
    "POST",
    { confirm: "release-tps-en", lease_id: leaseId },
    { bridgeAuth: true },
  );
export const previewDevdDeviceChargeControl = (
  baseUrl: string,
  deviceId: string,
  leaseId: string | null,
  input: ManualChargePreviewRequest,
) =>
  requestWithBody<ChargeControlDetail>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/charge-control/preview`,
    "POST",
    {
      ...input,
      lease_id: leaseId ?? undefined,
    },
    { bridgeAuth: true },
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
export const setDevdManualChargeControl = (
  baseUrl: string,
  deviceId: string,
  leaseId: string | null,
  input: ManualChargeControlRequest,
) =>
  requestWithBody<ChargeControlDetail>(
    baseUrl,
    `/api/v1/devices/${encodeURIComponent(deviceId)}/control/manual-charge`,
    "POST",
    {
      ...input,
      lease_id: leaseId ?? undefined,
    },
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
  if (
    error &&
    typeof error === "object" &&
    "envelope" in error &&
    isApiError(error.envelope)
  ) {
    return error.envelope;
  }
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

function isApiError(value: unknown): value is ApiErrorEnvelope["error"] {
  return Boolean(
    value &&
      typeof value === "object" &&
      "code" in value &&
      typeof value.code === "string" &&
      "message" in value &&
      typeof value.message === "string" &&
      "retryable" in value &&
      typeof value.retryable === "boolean",
  );
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
  const advancedPower = buildAdvancedPowerDefaults(ratedVoutMv);
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
      power_path: "auto",
    },
    charge_capabilities: defaultMockChargeCapabilities(),
    advanced_power: advancedPower,
    advanced_power_capabilities: buildAdvancedPowerCapabilities(ratedVoutMv),
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

function defaultMockChargeCapabilities(): ChargeCapabilities {
  return {
    target_voltage_mv: 16_800,
    normal_current_ma: 500,
    dc_derated_current_ma: 100,
    dcin_input_limit_ma: 1_000,
    max_output_current_ma: 3_500,
    usb_pd_high_power_min_voltage_mv: 9_000,
    usb_pd_high_power_max_voltage_mv: 20_000,
    usb_pd_high_power_min_power_mw: 20_000,
    loop_start_max_power_without_confirm_w10: 20,
    loop_stop_power_latched_w10: 30,
    loop_telemetry_miss_limit: 2,
    supported_power_paths: ["auto", "dcin", "usbc"],
    auto_path_priority: ["usbc", "dcin", "usbc"],
  };
}

function defaultMockChargeControl(): ChargeControlSummary {
  return {
    mode: "auto",
    manual_active: false,
    takeover: false,
    stop_inhibit: false,
    last_stop_reason: null,
    requested_power_path: "auto",
    bound_power_path: null,
    start_state: "idle",
    output_power_w10: 0,
    power_telemetry_fresh: true,
  };
}

function mockStatusForBaseUrl(baseUrl: string): UpsStatus {
  if (!mockStatusByBaseUrl.has(baseUrl)) {
    const baseStatus = getMockStatus(baseUrl);
    mockStatusByBaseUrl.set(baseUrl, {
      ...baseStatus,
      charge_control: baseStatus.charge_control ?? defaultMockChargeControl(),
    });
  }
  return mockStatusByBaseUrl.get(baseUrl)!;
}

function manualChargeCurrentMa(
  speed: DeviceSettings["manual_charge"]["speed"],
): number {
  if (speed === "ma_100") return 100;
  if (speed === "ma_1000") return 1_000;
  return 500;
}

function manualChargePreviewFromSettings(
  settings: DeviceSettings,
): ManualChargePreviewRequest {
  return {
    target: settings.manual_charge.target,
    current_ma: manualChargeCurrentMa(settings.manual_charge.speed),
    timer_minutes: settings.manual_charge.timer_h * 60,
    power_path: settings.manual_charge.power_path ?? "auto",
  };
}

function mockChargeTelemetry(
  baseUrl: string,
  status = mockStatusForBaseUrl(baseUrl),
  settings = mockSettingsForBaseUrl(baseUrl),
) {
  const inputSource = status.input.source ?? "unknown";
  return {
    input_source: inputSource,
    policy_target_ichg_ma: status.charger.policy_target_ichg_ma ?? null,
    ibat_actual_ma: status.charger.ibat_ma ?? null,
    target_voltage_mv:
      settings.charge_capabilities?.target_voltage_mv ?? 16_800,
    iindpm_ma: status.charger.limit_active
      ? status.charger.limit_threshold_ma ?? null
      : null,
    vindpm_mv: resolvePreTpsVinMv(status.input) ?? null,
    output_power_w10: status.charge_control?.output_power_w10 ?? 0,
    power_telemetry_fresh: status.charge_control?.power_telemetry_fresh ?? true,
    input_limit_summary:
      inputSource === "dcin"
        ? `${settings.charge_capabilities?.dcin_input_limit_ma ?? 1_000} mA DCIN`
        : "PD-qualified USB-C input",
    output_limit_summary: `${settings.charge_capabilities?.max_output_current_ma ?? 3_500} mA max output`,
  };
}

function mockChargeControlSummaryFromDetail(
  detail: ChargeControlDetail,
): ChargeControlSummary {
  return {
    mode: detail.summary.mode,
    manual_active: detail.summary.manual_active,
    takeover: detail.summary.takeover,
    stop_inhibit: detail.summary.stop_inhibit,
    last_stop_reason: detail.summary.last_stop_reason,
    requested_power_path: detail.readiness.planned_path.requested,
    bound_power_path: detail.readiness.planned_path.bound,
    start_state: detail.readiness.state,
    output_power_w10: detail.telemetry.output_power_w10,
    power_telemetry_fresh: detail.telemetry.power_telemetry_fresh,
  };
}

function mockChargeControlDetailForBaseUrl(baseUrl: string): ChargeControlDetail {
  const status = mockStatusForBaseUrl(baseUrl);
  const settings = mockSettingsForBaseUrl(baseUrl);
  const summary = status.charge_control ?? defaultMockChargeControl();
  const currentPath =
    summary.bound_power_path ?? summary.requested_power_path ?? "auto";
  return {
    summary: {
      mode: summary.mode,
      manual_active: summary.manual_active,
      takeover: summary.takeover,
      stop_inhibit: summary.stop_inhibit,
      last_stop_reason: summary.last_stop_reason,
      remaining_minutes:
        summary.manual_active && settings.manual_charge.timer_h
          ? settings.manual_charge.timer_h * 60
          : null,
      loop_override_active:
        summary.manual_active && currentPath === "usbc" && summary.mode === "manual",
    },
    readiness: {
      state: summary.manual_active ? "running" : summary.start_state,
      action: summary.manual_active
        ? "stop"
        : summary.start_state === "confirm_required"
          ? "confirm_loop"
          : summary.start_state === "blocked"
            ? "none"
            : "start",
      planned_path: {
        requested: summary.requested_power_path,
        bound: summary.bound_power_path,
        binding_reason:
          currentPath === "dcin"
            ? "auto_dcin_fallback"
            : currentPath === "usbc"
              ? "explicit_usbc"
              : null,
      },
      block:
        summary.start_state === "blocked"
          ? {
              code: "blocked_unknown",
              message: "Manual charge is currently blocked.",
            }
          : null,
      loop_override: {
        required: summary.start_state === "confirm_required",
        active:
          summary.manual_active && currentPath === "usbc" && summary.mode === "manual",
        allowed_guards: [
          "low_power_start_gate",
          "telemetry_miss_latch",
          "high_output_stop_latch",
        ],
      },
    },
    telemetry: mockChargeTelemetry(baseUrl, status, settings),
    evidence: [
      {
        source: "policy.state",
        code: "charger_state",
        label: "Charger state",
        value: status.charger.state ?? null,
      },
      {
        source: "charger.vbat_present",
        code: "vbat_present",
        label: "VBAT present",
        value: status.charger.vbat_present,
      },
      {
        source: "battery.charge_fet_on",
        code: "charge_fet",
        label: "Charge FET",
        value: status.battery.charge_fet_on ?? null,
      },
      {
        source: "output_power_w10",
        code: "output_power",
        label: "Output power",
        value: summary.output_power_w10,
      },
    ],
  };
}

function updateMockManualChargePrefs(baseUrl: string, body: unknown) {
  if (!body || typeof body !== "object") return;
  const current = mockSettingsForBaseUrl(baseUrl);
  const next = body as Partial<DeviceSettings["manual_charge"]>;
  mockSettingsByBaseUrl.set(baseUrl, {
    ...current,
    manual_charge: {
      ...current.manual_charge,
      ...next,
      power_path: next.power_path ?? current.manual_charge.power_path ?? "auto",
    },
  });
  const currentStatus = mockStatusForBaseUrl(baseUrl);
  mockStatusByBaseUrl.set(baseUrl, {
    ...currentStatus,
    charge_control: {
      ...(currentStatus.charge_control ?? defaultMockChargeControl()),
      requested_power_path:
        next.power_path ??
        current.manual_charge.power_path ??
        currentStatus.charge_control?.requested_power_path ??
        "auto",
    },
  });
}

function previewMockManualChargeControl(
  baseUrl: string,
  body: unknown,
): ChargeControlDetail {
  const settings = mockSettingsForBaseUrl(baseUrl);
  const status = mockStatusForBaseUrl(baseUrl);
  const input = (body ?? {}) as Partial<ManualChargePreviewRequest>;
  const requested = input.power_path ?? settings.manual_charge.power_path ?? "auto";
  const bound = requested === "auto" ? "dcin" : requested;
  const outputPowerW10 = status.charge_control?.output_power_w10 ?? 0;
  const powerFresh = status.charge_control?.power_telemetry_fresh ?? true;
  const confirmRequired =
    bound === "usbc" &&
    (!powerFresh || outputPowerW10 === null || outputPowerW10 >= 20);
  const blockedFull = status.battery.soc_pct === 100;
  return {
    summary: {
      mode: status.charge_control?.mode ?? "auto",
      manual_active: status.charge_control?.manual_active ?? false,
      takeover: status.charge_control?.takeover ?? false,
      stop_inhibit: status.charge_control?.stop_inhibit ?? false,
      last_stop_reason: status.charge_control?.last_stop_reason ?? null,
      remaining_minutes: status.charge_control?.manual_active
        ? input.timer_minutes ?? settings.manual_charge.timer_h * 60
        : null,
      loop_override_active: false,
    },
    readiness: {
      state: blockedFull ? "blocked" : confirmRequired ? "confirm_required" : "ready",
      action: blockedFull ? "none" : confirmRequired ? "confirm_loop" : "start",
      planned_path: {
        requested,
        bound,
        binding_reason:
          requested === "auto"
            ? "auto_dcin_fallback"
            : requested === "dcin"
              ? "explicit_dcin"
              : "explicit_usbc",
      },
      block: blockedFull
        ? {
            code: "battery_full",
            message: "Battery is already full.",
          }
        : null,
      loop_override: {
        required: confirmRequired,
        active: false,
        allowed_guards: [
          "low_power_start_gate",
          "telemetry_miss_latch",
          "high_output_stop_latch",
        ],
      },
    },
    telemetry: {
      ...mockChargeTelemetry(baseUrl, status, settings),
      policy_target_ichg_ma:
        input.current_ma ?? manualChargeCurrentMa(settings.manual_charge.speed),
    },
    evidence: blockedFull
      ? [
          {
            source: "policy.full_reason",
            code: "battery_full",
            label: "Policy full reason",
            value: "full_reached",
          },
        ]
      : [
          {
            source: "output_power_w10",
            code: "output_power",
            label: "Output power",
            value: outputPowerW10,
          },
          {
            source: "power_telemetry_fresh",
            code: "power_telemetry_fresh",
            label: "Power telemetry fresh",
            value: powerFresh,
          },
        ],
  };
}

function updateMockManualChargeControl(
  baseUrl: string,
  body: unknown,
): ManualChargeControlResponse {
  const currentStatus = mockStatusForBaseUrl(baseUrl);
  const currentSettings = mockSettingsForBaseUrl(baseUrl);
  const input = (body ?? {}) as Partial<ManualChargeControlRequest>;
  const currentChargeControl =
    currentStatus.charge_control ?? defaultMockChargeControl();
  const action = input.action === "stop" ? "stop" : "start";
  const requestedPowerPath =
    currentSettings.manual_charge.power_path ??
    currentChargeControl.requested_power_path ??
    "auto";
  if (action === "start") {
    const needsConfirm =
      requestedPowerPath === "usbc" &&
      !input.confirm_loop &&
      (currentChargeControl.output_power_w10 === null ||
        currentChargeControl.output_power_w10 >= 20);
    if (needsConfirm) {
      const detail = previewMockManualChargeControl(
        baseUrl,
        manualChargePreviewFromSettings(currentSettings),
      );
      throw new MainsAegisApiError({
        code: "loop_confirmation_required",
        message: "USB-C path requires loopback confirmation before manual charge can start",
        retryable: false,
        details: detail,
      });
    }
  }
  const nextChargeControl: ChargeControlSummary =
    action === "stop"
      ? {
          ...currentChargeControl,
          mode: "auto",
          manual_active: false,
          takeover: false,
          stop_inhibit: false,
          bound_power_path: null,
          start_state: "stopped",
        }
      : {
          ...currentChargeControl,
          mode: "manual",
          manual_active: true,
          takeover: true,
          stop_inhibit: false,
          requested_power_path: requestedPowerPath,
          bound_power_path:
            requestedPowerPath === "auto" ? "dcin" : requestedPowerPath,
          start_state: "running",
          output_power_w10: requestedPowerPath === "usbc" ? 24 : 0,
          power_telemetry_fresh: true,
          last_stop_reason: null,
        };
  mockStatusByBaseUrl.set(baseUrl, {
    ...currentStatus,
    charge_control: nextChargeControl,
  });
  return mockChargeControlDetailForBaseUrl(baseUrl);
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
      input_uvlo_cutoff_mv:
        typeof next.input_uvlo_cutoff_mv === "number"
          ? next.input_uvlo_cutoff_mv
          : current.advanced_power.input_uvlo_cutoff_mv,
      input_uvlo_recover_mv:
        typeof next.input_uvlo_recover_mv === "number"
          ? next.input_uvlo_recover_mv
          : current.advanced_power.input_uvlo_recover_mv,
      input_uvlo_required_samples:
        typeof next.input_uvlo_required_samples === "number"
          ? next.input_uvlo_required_samples
          : current.advanced_power.input_uvlo_required_samples,
      source_limited_enter_delta_ma:
        typeof next.source_limited_enter_delta_ma === "number"
          ? next.source_limited_enter_delta_ma
          : current.advanced_power.source_limited_enter_delta_ma,
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
