import { getMockIdentity, getMockNetwork, getMockStatus } from "../fixtures/mockDevices";
import type {
  ApiErrorEnvelope,
  DevdDevice,
  DefmtDecodeResult,
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
  if (value === "" || value === "same-origin" || value === "devd") return "";
  if (value.startsWith("/")) return "";
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
      details: { path, status: response.status, statusText: response.statusText, responseText: responseText.slice(0, 512) },
    });
  }

  return payload as T;
}

function parseJsonPayload<T>(text: string): T | ApiErrorEnvelope | null {
  if (!text) return null;
  try {
    return JSON.parse(text) as T | ApiErrorEnvelope;
  } catch {
    return null;
  }
}

function describeHttpFailure(response: Response, path: string, responseText: string): string {
  if (path === "/api/v1/defmt/decode" && response.status >= 500) {
    return "defmt decode API is unavailable. Check that the Web dev server proxies /api to the running mains-aegis-devd instance.";
  }
  const statusText = response.statusText || "request failed";
  const body = responseText.trim();
  return body ? `${statusText}: ${body.slice(0, 160)}` : statusText;
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

export type DevdSerialSession = {
  connected: boolean;
  protocol: string;
  status?: UpsStatus | null;
  logs: SerialLogEntry[];
  trace: SerialTraceEntry[];
  safeSettings: SafeSettingsState;
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

type DevdSerialSessionOptions = {
  logsLimit?: number;
  traceLimit?: number;
};

function devdSerialSessionPath(options: DevdSerialSessionOptions = {}) {
  const params = new URLSearchParams();
  if (options.logsLimit !== undefined) params.set("logs_limit", String(options.logsLimit));
  if (options.traceLimit !== undefined) params.set("trace_limit", String(options.traceLimit));
  const query = params.toString();
  return `/api/v1/serial/session${query ? `?${query}` : ""}`;
}

export const getDevdSerialSession = (baseUrl: string, options?: DevdSerialSessionOptions) =>
  requestJson<DevdSerialSession>(baseUrl, devdSerialSessionPath(options));

export function subscribeDevdSerialEvents(
  baseUrl: string,
  callbacks: {
    onEvent: (event: DevdSerialEvent) => void;
    onError: (event: Event) => void;
  },
): DevdSerialEventStream {
  const eventSource = new EventSource(`${baseUrl}/api/v1/serial/events`);
  const handleEvent = (event: Event) => {
    callbacks.onEvent(JSON.parse((event as MessageEvent<string>).data) as DevdSerialEvent);
  };
  eventSource.addEventListener("serial_trace", handleEvent);
  eventSource.addEventListener("serial_log", handleEvent);
  eventSource.addEventListener("serial_status", handleEvent);
  eventSource.addEventListener("monitor", handleEvent);
  eventSource.onerror = callbacks.onError;
  return { close: () => eventSource.close() };
}

export const sendDevdWifiConfig = (baseUrl: string, deviceId: string, input: { ssid: string; psk: string }) =>
  requestWithBody<unknown>(baseUrl, "/api/v1/wifi-config", "POST", { ...input, device_id: deviceId });
export const clearDevdWifiConfig = (baseUrl: string, deviceId: string) =>
  requestWithBody<unknown>(baseUrl, `/api/v1/wifi-config?device_id=${encodeURIComponent(deviceId)}`, "DELETE");
export const setDevdLogLevel = (baseUrl: string, deviceId: string, level: SafeSettingsState["log_level"]) =>
  requestWithBody<unknown>(baseUrl, "/api/v1/settings/log-level", "POST", { level, device_id: deviceId });
export const setDevdManualChargePrefs = (baseUrl: string, deviceId: string, prefs: SafeSettingsState["manual_charge"]) =>
  requestWithBody<unknown>(baseUrl, "/api/v1/settings/manual-charge", "POST", { ...prefs, device_id: deviceId });

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
export const loadFirmwareCatalogFromUrl = (url: string) => requestJson<FirmwareCatalog>("", url);
export const decodeDefmtFrame = (input: { elf_path: string; frame_hex: string }, baseUrl = "") =>
  requestWithBody<DefmtDecodeResult>(baseUrl, "/api/v1/defmt/decode", "POST", input);
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
