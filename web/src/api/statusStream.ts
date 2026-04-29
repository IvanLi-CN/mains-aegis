import { isMockBaseUrl } from "./client";
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
): StatusStream {
  if (isMockBaseUrl(baseUrl)) {
    return { close: () => undefined };
  }

  const eventSource = new EventSource(`${baseUrl}/api/v1/status`);

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
