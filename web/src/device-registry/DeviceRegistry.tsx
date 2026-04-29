import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { getStatus, normalizeBaseUrl, probeDevice, toErrorEnvelope } from "../api/client";
import { subscribeStatusStream, type StatusStream } from "../api/statusStream";
import type { DeviceRecord, DeviceTarget, ProbeResult } from "../api/types";
import { isDemoSeed, makeMockRecord, makeMockRecords, type DemoSeed } from "../fixtures/mockDevices";

type AddDeviceInput = {
  target: string;
  alias?: string;
  location?: string;
};

type AddDeviceResult =
  | { ok: true; record: DeviceRecord }
  | { ok: false; error: DeviceRecord["error"] };

type DeviceRegistryContextValue = {
  records: DeviceRecord[];
  addDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  removeDevice: (deviceId: string) => void;
  refreshDevice: (deviceId: string) => Promise<void>;
  resetDemo: () => void;
};

const STORAGE_KEY = "mains-aegis-web.devices.v1";
const DeviceRegistryContext = createContext<DeviceRegistryContextValue | null>(null);

export function DeviceRegistryProvider({ children }: { children: React.ReactNode }) {
  const seedRef = useRef<DemoSeed | null>(getDemoSeed());
  const [demoSeed, setDemoSeed] = useState<DemoSeed | null>(seedRef.current);
  const [records, setRecords] = useState<DeviceRecord[]>(() => loadInitialRecords(seedRef.current));
  const streams = useRef(new Map<string, StatusStream>());

  useEffect(() => {
    if (demoSeed) return;
    const targets = records.map((record) => record.target);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(targets));
  }, [demoSeed, records]);

  useEffect(() => {
    const syncSeedFromUrl = () => {
      const nextSeed = getDemoSeed();
      if (seedRef.current === nextSeed) return;
      seedRef.current = nextSeed;
      setDemoSeed(nextSeed);
      for (const stream of streams.current.values()) stream.close();
      streams.current.clear();
      if (!nextSeed) {
        setRecords(loadInitialRecords(null));
        return;
      }
      setRecords(makeMockRecords(nextSeed));
    };

    window.addEventListener("popstate", syncSeedFromUrl);
    return () => window.removeEventListener("popstate", syncSeedFromUrl);
  }, []);

  const refreshDevice = useCallback(async (deviceId: string) => {
    const target = records.find((record) => record.target.deviceId === deviceId)?.target;
    if (!target) return;
    if (target.mock) {
      setRecords((current) =>
        current.map((record) => (record.target.deviceId === deviceId ? makeMockRecord(record.target) : record)),
      );
      return;
    }

    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId ? { ...record, connectionState: "connecting", error: null } : record,
      ),
    );

    try {
      const result = await probeDevice(target.baseUrl);
      setRecords((current) => {
        const previous = current.find((record) => record.target.deviceId === deviceId);
        if (!previous) return current;
        const streamState = result.identity.capabilities.sse && previous.streamState !== "polling" ? "idle" : "polling";
        return upsertRecord(current, recordFromProbe(target, result, "online", streamState));
      });
    } catch (error) {
      const envelope = toErrorEnvelope(error);
      setRecords((current) =>
        current.map((record) =>
          record.target.deviceId === deviceId
            ? {
                ...record,
                connectionState: envelope.retryable ? "offline" : "error",
                streamState: "polling",
                error: envelope,
                lastUpdated: new Date().toISOString(),
              }
            : record,
        ),
      );
    }
  }, [records]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      for (const record of records) {
        if (!streams.current.has(record.target.deviceId)) {
          void refreshDevice(record.target.deviceId);
        }
      }
    }, 10000);
    return () => window.clearInterval(interval);
  }, [records, refreshDevice]);

  useEffect(() => {
    for (const record of records) {
      if (
        record.target.mock ||
        !record.identity?.capabilities.sse ||
        record.streamState !== "idle" ||
        streams.current.has(record.target.deviceId)
      ) {
        continue;
      }

      const subscription = subscribeStatusStream(record.target.baseUrl, {
        onStatus: (status) => {
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId
                ? {
                    ...candidate,
                    status,
                    connectionState: "online",
                    streamState: "streaming",
                    error: null,
                    lastUpdated: new Date().toISOString(),
                  }
                : candidate,
            ),
          );
        },
        onHeartbeat: () => {
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId
                ? { ...candidate, streamState: "streaming", lastUpdated: new Date().toISOString() }
                : candidate,
            ),
          );
        },
        onError: () => {
          subscription.close();
          streams.current.delete(record.target.deviceId);
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId
                ? { ...candidate, streamState: "polling" }
                : candidate,
            ),
          );
          void getStatus(record.target.baseUrl)
            .then((status) => {
              setRecords((current) =>
                current.map((candidate) =>
                  candidate.target.deviceId === record.target.deviceId
                    ? {
                        ...candidate,
                        status,
                        connectionState: "online",
                        streamState: "polling",
                        error: null,
                        lastUpdated: new Date().toISOString(),
                      }
                    : candidate,
                ),
              );
            })
            .catch((error) => {
              const envelope = toErrorEnvelope(error);
              setRecords((current) =>
                current.map((candidate) =>
                  candidate.target.deviceId === record.target.deviceId
                    ? {
                        ...candidate,
                        connectionState: envelope.retryable ? "offline" : "error",
                        streamState: "error",
                        error: envelope,
                        lastUpdated: new Date().toISOString(),
                      }
                    : candidate,
                ),
              );
            });
        },
      });

      streams.current.set(record.target.deviceId, subscription);
    }

    for (const [deviceId, stream] of streams.current.entries()) {
      if (!records.some((record) => record.target.deviceId === deviceId)) {
        stream.close();
        streams.current.delete(deviceId);
      }
    }
  }, [records]);

  useEffect(() => {
    return () => {
      for (const stream of streams.current.values()) stream.close();
      streams.current.clear();
    };
  }, []);

  const addDevice = useCallback(async (input: AddDeviceInput): Promise<AddDeviceResult> => {
    const baseUrl = normalizeBaseUrl(input.target);

    try {
      const result = await probeDevice(baseUrl);
      const target: DeviceTarget = {
        deviceId: result.identity.device_id,
        baseUrl,
        alias: input.alias?.trim() || result.identity.hostname,
        location: input.location?.trim() || "Unassigned",
        addedAt: new Date().toISOString(),
      };
      const record = recordFromProbe(target, result, "online", result.identity.capabilities.sse ? "idle" : "polling");
      setRecords((current) => upsertRecord(current, record));
      return { ok: true, record };
    } catch (error) {
      return { ok: false, error: toErrorEnvelope(error) };
    }
  }, []);

  const removeDevice = useCallback((deviceId: string) => {
    streams.current.get(deviceId)?.close();
    streams.current.delete(deviceId);
    setRecords((current) => current.filter((record) => record.target.deviceId !== deviceId));
  }, []);

  const resetDemo = useCallback(() => {
    for (const stream of streams.current.values()) stream.close();
    streams.current.clear();
    setRecords(makeMockRecords("default"));
  }, []);

  const value = useMemo(
    () => ({ records, addDevice, removeDevice, refreshDevice, resetDemo }),
    [records, addDevice, removeDevice, refreshDevice, resetDemo],
  );

  return <DeviceRegistryContext.Provider value={value}>{children}</DeviceRegistryContext.Provider>;
}

export function useDeviceRegistry(): DeviceRegistryContextValue {
  const context = useContext(DeviceRegistryContext);
  if (!context) throw new Error("useDeviceRegistry must be used inside DeviceRegistryProvider");
  return context;
}

function getDemoSeed(): DemoSeed | null {
  const seed = new URLSearchParams(window.location.search).get("seed");
  return isDemoSeed(seed) ? seed : null;
}

function loadInitialRecords(seed: DemoSeed | null): DeviceRecord[] {
  if (seed) return makeMockRecords(seed);

  const stored = localStorage.getItem(STORAGE_KEY);
  if (!stored) return makeMockRecords("default");

  try {
    const targets = JSON.parse(stored) as DeviceTarget[];
    if (!Array.isArray(targets)) return makeMockRecords("default");
    return targets.map((target) => {
      if (target.mock) return makeMockRecord(target);
      return {
        target,
        identity: null,
        network: null,
        status: null,
        connectionState: "connecting",
        streamState: "idle",
        error: null,
        lastUpdated: null,
      };
    });
  } catch {
    return makeMockRecords("default");
  }
}

function recordFromProbe(
  target: DeviceTarget,
  result: ProbeResult,
  connectionState: DeviceRecord["connectionState"],
  streamState: DeviceRecord["streamState"],
): DeviceRecord {
  return {
    target,
    identity: result.identity,
    network: result.network,
    status: result.status,
    connectionState,
    streamState,
    error: null,
    lastUpdated: new Date().toISOString(),
  };
}

function upsertRecord(records: DeviceRecord[], record: DeviceRecord): DeviceRecord[] {
  const next = records.filter((candidate) => candidate.target.deviceId !== record.target.deviceId);
  next.push(record);
  return next;
}
