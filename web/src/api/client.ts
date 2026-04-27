import { getMockIdentity, getMockNetwork, getMockStatus } from "../fixtures/mockDevices";
import type { ApiErrorEnvelope, Identity, NetworkSummary, ProbeResult, UpsStatus } from "./types";

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
  if (isMockBaseUrl(baseUrl)) {
    return requestMock<T>(baseUrl, path);
  }

  const response = await fetch(`${baseUrl}${path}`, {
    method: "GET",
    headers: {
      Accept: "application/json",
    },
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
