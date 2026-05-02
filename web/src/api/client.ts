import { getMockIdentity, getMockNetwork, getMockStatus } from "../fixtures/mockDevices";
import type {
  ApiErrorEnvelope,
  DevdDevice,
  FirmwareCatalog,
  Identity,
  NetworkSummary,
  ProbeResult,
  SafeSettingsState,
  SerialLogEntry,
  SerialTraceEntry,
  UpsStatus,
} from "./types";

export class MainsAegisApiError extends Error {
  envelope: ApiErrorEnvelope["error"];

  constructor(envelope: ApiErrorEnvelope["error"]) {
    super(envelope.message);
    this.name = "MainsAegisApiError";
    this.envelope = envelope;
  }
}

export const isMockBaseUrl = (baseUrl: string) => baseUrl.startsWith("mock:");

export function normalizeBaseUrl(input: string): string {
  const value = input.trim();
  if (value.startsWith("mock:")) return value;
  if (/^https?:\/\//i.test(value)) return value.replace(/\/+$/, "");
  return `http://${value.replace(/\/+$/, "")}`;
}

async function requestJson<T>(baseUrl: string, path: string): Promise<T> {
  return requestWithBody<T>(baseUrl, path, "GET");
}

async function requestWithBody<T>(baseUrl: string, path: string, method: "GET" | "POST" | "DELETE", body?: unknown): Promise<T> {
  if (isMockBaseUrl(baseUrl)) {
    return requestMock<T>(baseUrl, path);
  }

  const response = await fetch(`${baseUrl}${path}`, {
    method,
    headers: {
      Accept: "application/json",
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

  const payload = (await response.json().catch(() => null)) as T | ApiErrorEnvelope | null;

  if (!response.ok) {
    if (payload && typeof payload === "object" && "error" in payload) {
      throw new MainsAegisApiError(payload.error);
    }
    throw new MainsAegisApiError({
      code: `http_${response.status}`,
      message: response.statusText || "request failed",
      retryable: response.status >= 500,
      details: null,
    });
  }

  return payload as T;
}

function requestMock<T>(baseUrl: string, path: string): Promise<T> {
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
  throw new MainsAegisApiError({
    code: "not_found",
    message: "mock endpoint not found",
    retryable: false,
    details: { path },
  });
}

export const ping = (baseUrl: string) => requestJson<{ ok: true }>(baseUrl, "/api/v1/ping");
export const getIdentity = (baseUrl: string) => requestJson<Identity>(baseUrl, "/api/v1/identity");
export const getNetwork = (baseUrl: string) => requestJson<NetworkSummary>(baseUrl, "/api/v1/network");
export const getStatus = (baseUrl: string) => requestJson<UpsStatus>(baseUrl, "/api/v1/status");

export type AdapterSerialSession = {
  connected: boolean;
  protocol: string;
  logs: SerialLogEntry[];
  trace: SerialTraceEntry[];
  safeSettings: SafeSettingsState;
};

type AdapterSerialSessionOptions = {
  logsLimit?: number;
  traceLimit?: number;
};

function adapterSerialSessionPath(options: AdapterSerialSessionOptions = {}) {
  const params = new URLSearchParams();
  if (options.logsLimit !== undefined) params.set("logs_limit", String(options.logsLimit));
  if (options.traceLimit !== undefined) params.set("trace_limit", String(options.traceLimit));
  const query = params.toString();
  return `/api/v1/serial/session${query ? `?${query}` : ""}`;
}

export const getAdapterSerialSession = (baseUrl: string, options?: AdapterSerialSessionOptions) =>
  requestJson<AdapterSerialSession>(baseUrl, adapterSerialSessionPath(options));
export const sendAdapterWifiConfig = (baseUrl: string, input: { ssid: string; psk: string }) =>
  requestWithBody<unknown>(baseUrl, "/api/v1/wifi-config", "POST", input);
export const clearAdapterWifiConfig = (baseUrl: string) => requestWithBody<unknown>(baseUrl, "/api/v1/wifi-config", "DELETE");
export const setAdapterLogLevel = (baseUrl: string, level: SafeSettingsState["log_level"]) =>
  requestWithBody<unknown>(baseUrl, "/api/v1/settings/log-level", "POST", { level });
export const setAdapterManualChargePrefs = (baseUrl: string, prefs: SafeSettingsState["manual_charge"]) =>
  requestWithBody<unknown>(baseUrl, "/api/v1/settings/manual-charge", "POST", prefs);

export async function probeDevice(baseUrl: string): Promise<ProbeResult> {
  await ping(baseUrl);
  const identity = await getIdentity(baseUrl);
  const network = await getNetwork(baseUrl);
  const status = await getStatus(baseUrl);
  return { identity, network, status };
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

export const loadBundledFirmwareCatalog = () => requestJson<FirmwareCatalog>("", "/firmware/firmware-catalog.json");
export const listDevdDevices = (baseUrl = "") => requestJson<{ devices: DevdDevice[] }>(baseUrl, "/api/v1/devices");
export const scanDevdDevices = (baseUrl = "") => requestWithBody<{ devices: DevdDevice[] }>(baseUrl, "/api/v1/devices/scan", "POST");
export const bindDevdDevice = (deviceId: string, alias?: string, baseUrl = "") =>
  requestWithBody<DevdDevice>(baseUrl, `/api/v1/devices/${encodeURIComponent(deviceId)}/bind`, "POST", { alias });
export const connectDevdDevice = (deviceId: string, baseUrl = "") =>
  requestWithBody<DevdDevice>(baseUrl, `/api/v1/devices/${encodeURIComponent(deviceId)}/connect`, "POST");
export const disconnectDevdDevice = (deviceId: string, baseUrl = "") =>
  requestWithBody<DevdDevice>(baseUrl, `/api/v1/devices/${encodeURIComponent(deviceId)}/disconnect`, "POST");
export const selectDevdArtifact = (deviceId: string, input: { artifact_id?: string; manifest_path?: string }, baseUrl = "") =>
  requestWithBody<unknown>(baseUrl, `/api/v1/devices/${encodeURIComponent(deviceId)}/artifact`, "POST", input);
export const flashDevdDevice = (deviceId: string, input: { artifact_id?: string; dry_run?: boolean }, baseUrl = "") =>
  requestWithBody<unknown>(baseUrl, `/api/v1/devices/${encodeURIComponent(deviceId)}/flash`, "POST", input);
