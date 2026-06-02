import { bridgeAuthToken, isMockBaseUrl } from "./client";
import type { UpsStatus } from "./types";

export type StatusStream = {
  close: () => void;
};

export function subscribeStatusStream(
  baseUrl: string,
  callbacks: {
    onStatus: (status: UpsStatus) => void;
    onHeartbeat: () => void;
    onError: (error: Event) => void;
  },
  options: { bridgeAuth?: boolean } = {},
): StatusStream {
  if (isMockBaseUrl(baseUrl)) {
    return { close: () => undefined };
  }

  const params = new URLSearchParams();
  const token = options.bridgeAuth ? bridgeAuthToken(baseUrl) : null;
  if (token) params.set("bridge_token", token);
  const query = params.toString();
  const eventSource = new EventSource(`${baseUrl}/api/v1/status${query ? `?${query}` : ""}`);

  eventSource.addEventListener("status", (event) => {
    callbacks.onStatus(JSON.parse((event as MessageEvent<string>).data) as UpsStatus);
  });

  eventSource.addEventListener("heartbeat", () => {
    callbacks.onHeartbeat();
  });

  eventSource.onerror = (event) => {
    callbacks.onError(event);
  };

  return {
    close: () => eventSource.close(),
  };
}
