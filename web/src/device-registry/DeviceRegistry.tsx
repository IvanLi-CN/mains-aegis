import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import {
  clearAdapterWifiConfig,
  getAdapterSerialSession,
  getStatus,
  normalizeBaseUrl,
  probeDevice,
  sendAdapterWifiConfig,
  setAdapterLogLevel,
  setAdapterManualChargePrefs,
  toErrorEnvelope,
  type AdapterSerialSession,
} from "../api/client";
import { subscribeStatusStream, type StatusStream } from "../api/statusStream";
import type { DeviceRecord, DeviceTarget, ProbeResult, SafeSettingsState, SerialLogEntry, SerialTraceEntry } from "../api/types";
import { isDemoSeed, makeMockRecord, makeMockRecords, makeMockUsbSerialRecord, type DemoSeed } from "../fixtures/mockDevices";
import {
  errorFromSerialFailure,
  isWebSerialSupported,
  type SerialFrame,
  type SerialLogFrame,
  type SerialStatusFrame,
  type SerialTraceEvent,
  WebSerialTransport,
} from "../serial/transport";

const ADAPTER_SERIAL_SESSION_LIMITS = {
  logsLimit: 200,
  traceLimit: 600,
};

type AddDeviceInput = {
  target: string;
  alias?: string;
  location?: string;
};

type AddDeviceResult =
  | { ok: true; record: DeviceRecord }
  | { ok: false; error: DeviceRecord["error"] };

type CommandResult = { ok: true } | { ok: false; error: DeviceRecord["error"] };

type WifiConfigInput = {
  ssid: string;
  psk: string;
};

type ManualChargePrefsInput = SafeSettingsState["manual_charge"];

type DeviceRegistryContextValue = {
  records: DeviceRecord[];
  addDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  addLocalAdapterDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  connectUsbSerialDevice: (input?: Pick<AddDeviceInput, "alias" | "location">) => Promise<AddDeviceResult>;
  attachMockUsbSerialDevice: () => AddDeviceResult;
  disconnectUsbSerialDevice: (deviceId: string) => Promise<void>;
  sendWifiConfig: (deviceId: string, input: WifiConfigInput) => Promise<CommandResult>;
  clearWifiConfig: (deviceId: string) => Promise<CommandResult>;
  setSerialLogLevel: (deviceId: string, level: SafeSettingsState["log_level"]) => Promise<CommandResult>;
  setManualChargePrefs: (deviceId: string, prefs: ManualChargePrefsInput) => Promise<CommandResult>;
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
  const serialSessions = useRef(new Map<string, WebSerialTransport>());

  useEffect(() => {
    if (demoSeed) return;
    const targets = records
      .filter((record) => record.target.transport !== "serial" && !record.target.mock)
      .map((record) => record.target);
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
      for (const session of serialSessions.current.values()) void session.close();
      serialSessions.current.clear();
      if (!nextSeed) {
        setRecords(loadInitialRecords(null));
        return;
      }
      setRecords(makeMockRecords(nextSeed));
    };

    window.addEventListener("popstate", syncSeedFromUrl);
    return () => window.removeEventListener("popstate", syncSeedFromUrl);
  }, []);

  const setRecordError = useCallback((deviceId: string, error: DeviceRecord["error"]) => {
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId
          ? {
              ...record,
              connectionState: error?.retryable ? "offline" : "error",
              streamState: "error",
              error,
              serial: record.serial ? { ...record.serial, connected: false } : record.serial,
              lastUpdated: new Date().toISOString(),
            }
          : record,
      ),
    );
  }, []);

  const setSerialCommandError = useCallback(
    (deviceId: string, error: DeviceRecord["error"]) => {
      if (!serialSessions.current.has(deviceId)) {
        let handledByAdapter = false;
        setRecords((current) =>
          current.map((record) => {
            if (record.target.deviceId !== deviceId || record.target.transport !== "adapter" || !record.serial) return record;
            handledByAdapter = true;
            return appendSerialLog(
              {
                ...record,
                connectionState: "online",
                streamState: "polling",
                error,
                serial: { ...record.serial, connected: true },
                lastUpdated: new Date().toISOString(),
              },
              serialLogFromFrame({
                type: "log",
                level: "error",
                target: "usb_http_adapter",
                message: `${error?.code ?? "adapter_error"}: ${error?.message ?? "USB HTTP adapter command failed"}`,
              }),
            );
          }),
        );
        if (!handledByAdapter) setRecordError(deviceId, error);
        return;
      }
      const log = serialLogFromFrame({
        type: "log",
        level: "error",
        target: "usb_cdc",
        message: `${error?.code ?? "serial_error"}: ${error?.message ?? "USB CDC command failed"}`,
      });
      setRecords((current) =>
        current.map((record) =>
          record.target.deviceId === deviceId
            ? appendSerialLog(
                {
                  ...record,
                  connectionState: "online",
                  streamState: "streaming",
                  error,
                  serial: record.serial ? { ...record.serial, connected: true } : record.serial,
                  lastUpdated: new Date().toISOString(),
                },
                log,
              )
            : record,
        ),
      );
    },
    [setRecordError],
  );

  const refreshDevice = useCallback(async (deviceId: string) => {
    const existing = records.find((record) => record.target.deviceId === deviceId);
    const target = existing?.target;
    if (!target) return;
    if (target.mock) {
      setRecords((current) =>
        current.map((record) =>
          record.target.deviceId === deviceId
            ? record.target.transport === "serial"
              ? {
                  ...record,
                  connectionState: "online",
                  streamState: "streaming",
                  error: null,
                  serial: record.serial ? { ...record.serial, connected: true } : record.serial,
                  lastUpdated: new Date().toISOString(),
                }
              : makeMockRecord(record.target)
            : record,
        ),
      );
      return;
    }
    if (target.transport === "serial") {
      const session = serialSessions.current.get(deviceId);
      if (!session) {
        setRecords((current) =>
          current.map((record) =>
            record.target.deviceId === deviceId
              ? {
                  ...record,
                  connectionState: "offline",
                  streamState: "error",
                  error: {
                    code: "serial_disconnected",
                    message: "USB CDC device is not connected in this browser session",
                    retryable: true,
                    details: null,
                  },
                  serial: record.serial ? { ...record.serial, connected: false } : record.serial,
                  lastUpdated: new Date().toISOString(),
                }
              : record,
          ),
        );
        return;
      }
      setRecords((current) =>
        current.map((record) =>
          record.target.deviceId === deviceId ? { ...record, connectionState: "connecting", error: null } : record,
        ),
      );
      try {
        const status = await session.requestStatus();
        setRecords((current) =>
          current.map((record) =>
            record.target.deviceId === deviceId
              ? {
                  ...record,
                  status,
                  network: record.identity?.network ?? record.network,
                  connectionState: "online",
                  streamState: "streaming",
                  error: null,
                  lastUpdated: new Date().toISOString(),
                }
              : record,
          ),
        );
      } catch (error) {
        setRecordError(deviceId, errorFromSerialFailure(error));
      }
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
        if (target.transport === "adapter") {
          const streamState = result.identity.capabilities.sse && previous.streamState !== "polling" ? "idle" : "polling";
          const previousSerial = previous.serial;
          void getAdapterSerialSession(target.baseUrl, ADAPTER_SERIAL_SESSION_LIMITS)
            .then((session) => {
              setRecords((latest) =>
                latest.map((candidate) =>
                  candidate.target.deviceId === deviceId && candidate.target.transport === "adapter"
                    ? mergeAdapterSerial(candidate, session)
                    : candidate,
                ),
              );
            })
            .catch(() => undefined);
          return upsertRecord(
            current,
            {
              ...recordFromProbe(target, result, "online", streamState),
              serial: previousSerial
                ? {
                    ...previousSerial,
                    connected: true,
                    protocol: target.serialProtocol ?? previousSerial.protocol,
                  }
                : previousSerial,
            },
          );
        }
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
  }, [records, setRecordError]);

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
        record.target.transport === "serial" ||
        record.target.transport === "adapter" ||
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
      for (const session of serialSessions.current.values()) void session.close();
      serialSessions.current.clear();
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

  const addLocalAdapterDevice = useCallback(async (input: AddDeviceInput): Promise<AddDeviceResult> => {
    const baseUrl = normalizeBaseUrl(input.target);

    try {
      const result = await probeDevice(baseUrl);
      const session = await getAdapterSerialSession(baseUrl, ADAPTER_SERIAL_SESSION_LIMITS);
      const target: DeviceTarget = {
        deviceId: result.identity.device_id,
        baseUrl,
        alias: input.alias?.trim() || result.identity.hostname,
        location: input.location?.trim() || "Local adapter",
        addedAt: new Date().toISOString(),
        transport: "adapter",
        serialProtocol: session.protocol,
      };
      const record = recordFromAdapterProbe(target, result, session);
      setRecords((current) => upsertRecord(current, record));
      return { ok: true, record };
    } catch (error) {
      return { ok: false, error: toErrorEnvelope(error) };
    }
  }, []);

  const connectUsbSerialDevice = useCallback(
    async (input: Pick<AddDeviceInput, "alias" | "location"> = {}): Promise<AddDeviceResult> => {
      if (!isWebSerialSupported()) {
        return {
          ok: false,
          error: {
            code: "serial_unsupported",
            message: "This browser does not expose the Web Serial API",
            retryable: false,
            details: null,
          },
        };
      }

      let openedTransport: WebSerialTransport | null = null;
      try {
        let transportRef: WebSerialTransport | null = null;
        const pendingLogs: SerialLogEntry[] = [];
        const pendingTrace: SerialTraceEntry[] = [];
        const transport = await WebSerialTransport.request({
          onFrame: (frame) => {
            const deviceId = transportRef ? findSessionDeviceId(transportRef) : null;
            if (!deviceId) {
              if (frame.type === "log") pendingLogs.push(serialLogFromFrame(frame));
              if (frame.type === "error") {
                pendingLogs.push(
                  serialLogFromFrame({
                    type: "log",
                    level: "error",
                    target: "usb_cdc",
                    message: `${frame.error.code}: ${frame.error.message}`,
                  }),
                );
              }
              return;
            }
            handleSerialFrame(frame, deviceId);
          },
          onTrace: (entry) => {
            const deviceId = transportRef ? findSessionDeviceId(transportRef) : null;
            if (!deviceId) {
              pendingTrace.push(serialTraceFromEvent(entry));
              return;
            }
            appendSerialTraceToSession(entry, deviceId);
          },
          onClose: (error) => {
            if (!transportRef) return;
            const deviceId = findSessionDeviceId(transportRef);
            if (!deviceId) return;
            serialSessions.current.delete(deviceId);
            setRecordError(
              deviceId,
              error
                ? errorFromSerialFailure(error)
                : {
                    code: "serial_disconnected",
                    message: "USB CDC device disconnected",
                    retryable: true,
                    details: null,
              },
            );
          },
        });
        openedTransport = transport;
        transportRef = transport;
        const hello = await transport.hello();
        const status = await transport.requestStatus();
        const identity = hello.identity;
        const target: DeviceTarget = {
          deviceId: identity.device_id,
          baseUrl: `serial:${identity.device_id}`,
          alias: input.alias?.trim() || identity.hostname,
          location: input.location?.trim() || "USB",
          addedAt: new Date().toISOString(),
          transport: "serial",
          serialProtocol: hello.protocol,
        };
        serialSessions.current.set(identity.device_id, transport);
        openedTransport = null;
        const record = recordFromSerialProbe(
          target,
          { identity, network: identity.network, status },
          hello.protocol,
          [
            ...pendingLogs,
            serialLogFromFrame({
              type: "log",
              level: "info",
              target: "web",
              message: "USB CDC connected",
            }),
          ],
          pendingTrace,
        );
        setRecords((current) => upsertRecord(current, record));
        return { ok: true, record };
      } catch (error) {
        await openedTransport?.close().catch(() => undefined);
        return { ok: false, error: errorFromSerialFailure(error) };
      }
    },
    [setRecordError],
  );

  const attachMockUsbSerialDevice = useCallback((): AddDeviceResult => {
    const record = makeMockUsbSerialRecord();
    setRecords((current) => upsertRecord(current, record));
    return { ok: true, record };
  }, []);

  const disconnectUsbSerialDevice = useCallback(async (deviceId: string) => {
    const session = serialSessions.current.get(deviceId);
    serialSessions.current.delete(deviceId);
    await session?.close();
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId
          ? {
              ...record,
              connectionState: "offline",
              streamState: "idle",
              serial: record.serial ? { ...record.serial, connected: false } : record.serial,
              lastUpdated: new Date().toISOString(),
            }
          : record,
      ),
    );
  }, []);

  const sendWifiConfig = useCallback(async (deviceId: string, input: WifiConfigInput): Promise<CommandResult> => {
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    if (!record?.serial) return serialCommandUnavailable();
    if (record.target.mock) {
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(candidate, {
                wifi_configured: true,
                wifi_ssid: input.ssid,
              }, "wifi_config", `WiFi credentials saved for ${input.ssid}`)
            : candidate,
        ),
      );
      return { ok: true };
    }
    if (record.target.transport === "adapter") {
      try {
        await sendAdapterWifiConfig(record.target.baseUrl, input);
        await updateAdapterSerialSnapshot(record.target.deviceId, record.target.baseUrl);
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? updateSerialSettings(candidate, {
                  wifi_configured: true,
                  wifi_ssid: input.ssid,
                }, "wifi_config", `WiFi credentials saved for ${input.ssid}`)
              : candidate,
          ),
        );
        return { ok: true };
      } catch (error) {
        const envelope = toErrorEnvelope(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    }
    const session = serialSessions.current.get(deviceId);
    if (!session) return serialCommandUnavailable();
    try {
      await session.setWifiConfig(input.ssid, input.psk);
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(candidate, {
                wifi_configured: true,
                wifi_ssid: input.ssid,
              }, "wifi_config", `WiFi credentials saved for ${input.ssid}`)
            : candidate,
        ),
      );
      return { ok: true };
    } catch (error) {
      const envelope = errorFromSerialFailure(error);
      setSerialCommandError(deviceId, envelope);
      return { ok: false, error: envelope };
    }
  }, [records, setSerialCommandError]);

  const clearWifiConfig = useCallback(async (deviceId: string): Promise<CommandResult> => {
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    if (!record?.serial) return serialCommandUnavailable();
    if (record.target.mock) {
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(candidate, { wifi_configured: false, wifi_ssid: null }, "wifi_config", "WiFi credentials cleared")
            : candidate,
        ),
      );
      return { ok: true };
    }
    if (record.target.transport === "adapter") {
      try {
        await clearAdapterWifiConfig(record.target.baseUrl);
        await updateAdapterSerialSnapshot(record.target.deviceId, record.target.baseUrl);
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? updateSerialSettings(candidate, { wifi_configured: false, wifi_ssid: null }, "wifi_config", "WiFi credentials cleared")
              : candidate,
          ),
        );
        return { ok: true };
      } catch (error) {
        const envelope = toErrorEnvelope(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    }
    const session = serialSessions.current.get(deviceId);
    if (!session) return serialCommandUnavailable();
    try {
      await session.clearWifiConfig();
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(candidate, { wifi_configured: false, wifi_ssid: null }, "wifi_config", "WiFi credentials cleared")
            : candidate,
        ),
      );
      return { ok: true };
    } catch (error) {
      const envelope = errorFromSerialFailure(error);
      setSerialCommandError(deviceId, envelope);
      return { ok: false, error: envelope };
    }
  }, [records, setSerialCommandError]);

  const setSerialLogLevel = useCallback(async (deviceId: string, level: SafeSettingsState["log_level"]): Promise<CommandResult> => {
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    if (!record?.serial) return serialCommandUnavailable();
    if (record.target.transport === "adapter") {
      try {
        await setAdapterLogLevel(record.target.baseUrl, level);
        await updateAdapterSerialSnapshot(record.target.deviceId, record.target.baseUrl);
      } catch (error) {
        const envelope = toErrorEnvelope(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    } else if (!record.target.mock) {
      const session = serialSessions.current.get(deviceId);
      if (!session) return serialCommandUnavailable();
      try {
        await session.setLogLevel(level);
      } catch (error) {
        const envelope = errorFromSerialFailure(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    }
    setRecords((current) =>
      current.map((candidate) =>
        candidate.target.deviceId === deviceId
          ? updateSerialSettings(candidate, { log_level: level }, "usb_cdc", `Log level set to ${level}`)
          : candidate,
      ),
    );
    return { ok: true };
  }, [records, setSerialCommandError]);

  const setManualChargePrefs = useCallback(async (deviceId: string, prefs: ManualChargePrefsInput): Promise<CommandResult> => {
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    if (!record?.serial) return serialCommandUnavailable();
    if (record.target.transport === "adapter") {
      try {
        await setAdapterManualChargePrefs(record.target.baseUrl, prefs);
        await updateAdapterSerialSnapshot(record.target.deviceId, record.target.baseUrl);
      } catch (error) {
        const envelope = toErrorEnvelope(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    } else if (!record.target.mock) {
      const session = serialSessions.current.get(deviceId);
      if (!session) return serialCommandUnavailable();
      try {
        await session.setManualChargePrefs(prefs);
      } catch (error) {
        const envelope = errorFromSerialFailure(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    }
    setRecords((current) =>
      current.map((candidate) =>
        candidate.target.deviceId === deviceId
          ? updateSerialSettings(candidate, { manual_charge: prefs }, "manual_charge", "Manual charge preferences updated")
          : candidate,
      ),
    );
    return { ok: true };
  }, [records, setSerialCommandError]);

  function handleSerialFrame(frame: SerialFrame, deviceId: string | null) {
    if (frame.type === "status") {
      updateSerialStatus(frame, deviceId);
      return;
    }
    if (frame.type === "log") {
      appendSerialLogToSession(frame, deviceId);
      return;
    }
    if (frame.type === "error") {
      const requestLog = serialLogFromFrame({
        type: "log",
        level: "error",
        target: "usb_cdc",
        message: `${frame.error.code}: ${frame.error.message}`,
      });
      appendSerialLogToSession(requestLog, deviceId);
    }
  }

  function findSessionDeviceId(session: WebSerialTransport): string | null {
    for (const [deviceId, candidate] of serialSessions.current.entries()) {
      if (candidate === session) return deviceId;
    }
    return null;
  }

  function updateSerialStatus(frame: SerialStatusFrame, deviceId: string | null) {
    if (!deviceId) return;
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId && record.serial?.connected
          ? {
              ...record,
              status: frame.status,
              connectionState: "online",
              streamState: "streaming",
              error: null,
              lastUpdated: new Date().toISOString(),
            }
          : record,
      ),
    );
  }

  function appendSerialLogToSession(frame: SerialLogFrame | SerialLogEntry, deviceId: string | null) {
    if (!deviceId) return;
    const entry = "timestamp" in frame ? frame : serialLogFromFrame(frame);
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId && record.serial?.connected ? appendSerialLog(record, entry) : record,
      ),
    );
  }

  function appendSerialTraceToSession(entry: SerialTraceEvent | SerialTraceEntry, deviceId: string | null) {
    if (!deviceId) return;
    const trace = "timestamp" in entry ? entry : serialTraceFromEvent(entry);
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId && record.serial?.connected ? appendSerialTrace(record, trace) : record,
      ),
    );
  }

  async function updateAdapterSerialSnapshot(deviceId: string, baseUrl: string) {
    const session = await getAdapterSerialSession(baseUrl, ADAPTER_SERIAL_SESSION_LIMITS);
    setRecords((current) =>
      current.map((record) => (record.target.deviceId === deviceId && record.target.transport === "adapter" ? mergeAdapterSerial(record, session) : record)),
    );
  }

  const removeDevice = useCallback((deviceId: string) => {
    streams.current.get(deviceId)?.close();
    streams.current.delete(deviceId);
    void serialSessions.current.get(deviceId)?.close();
    serialSessions.current.delete(deviceId);
    setRecords((current) => current.filter((record) => record.target.deviceId !== deviceId));
  }, []);

  const resetDemo = useCallback(() => {
    for (const stream of streams.current.values()) stream.close();
    streams.current.clear();
    for (const session of serialSessions.current.values()) void session.close();
    serialSessions.current.clear();
    setRecords(makeMockRecords("default"));
  }, []);

  const value = useMemo(
    () => ({
      records,
      addDevice,
      addLocalAdapterDevice,
      connectUsbSerialDevice,
      attachMockUsbSerialDevice,
      disconnectUsbSerialDevice,
      sendWifiConfig,
      clearWifiConfig,
      setSerialLogLevel,
      setManualChargePrefs,
      removeDevice,
      refreshDevice,
      resetDemo,
    }),
    [
      records,
      addDevice,
      addLocalAdapterDevice,
      connectUsbSerialDevice,
      attachMockUsbSerialDevice,
      disconnectUsbSerialDevice,
      sendWifiConfig,
      clearWifiConfig,
      setSerialLogLevel,
      setManualChargePrefs,
      removeDevice,
      refreshDevice,
      resetDemo,
    ],
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
  if (!stored) return [];

  try {
    const targets = JSON.parse(stored) as DeviceTarget[];
    if (!Array.isArray(targets)) return [];
    return targets.filter((target) => !target.mock).map((target) => {
      return {
        target,
        identity: null,
        network: null,
        status: null,
        connectionState: target.transport === "serial" ? "offline" : "connecting",
        streamState: target.transport === "serial" ? "error" : "idle",
        error:
          target.transport === "serial"
            ? {
                code: "serial_reconnect_required",
                message: "USB CDC devices require a fresh browser permission grant",
                retryable: true,
                details: null,
              }
            : null,
        lastUpdated: null,
      };
    });
  } catch {
    return [];
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

function recordFromSerialProbe(
  target: DeviceTarget,
  result: ProbeResult,
  protocol: string,
  logs: SerialLogEntry[] = [],
  trace: SerialTraceEntry[] = [],
): DeviceRecord {
  return {
    target,
    identity: result.identity,
    network: result.network,
    status: result.status,
    connectionState: "online",
    streamState: "streaming",
    error: null,
    lastUpdated: new Date().toISOString(),
    serial: {
      connected: true,
      protocol,
      logs,
      trace,
      safeSettings: defaultSafeSettings(),
    },
  };
}

function recordFromAdapterProbe(target: DeviceTarget, result: ProbeResult, session: AdapterSerialSession): DeviceRecord {
  return mergeAdapterSerial(recordFromProbe(target, result, "online", "polling"), session);
}

function mergeAdapterSerial(record: DeviceRecord, session: AdapterSerialSession): DeviceRecord {
  return {
    ...record,
    target: {
      ...record.target,
      serialProtocol: session.protocol,
    },
    connectionState: session.connected ? "online" : record.connectionState,
    error: session.connected ? null : record.error,
    lastUpdated: new Date().toISOString(),
    serial: {
      connected: session.connected,
      protocol: session.protocol,
      logs: session.logs,
      trace: session.trace,
      safeSettings: session.safeSettings,
    },
  };
}

function upsertRecord(records: DeviceRecord[], record: DeviceRecord): DeviceRecord[] {
  const next = records.filter((candidate) => candidate.target.deviceId !== record.target.deviceId);
  next.push(record);
  return next;
}

function defaultSafeSettings(): SafeSettingsState {
  return {
    wifi_configured: null,
    wifi_ssid: null,
    log_level: "info",
    manual_charge: {
      target: "full_100",
      speed: "ma_500",
      timer_h: 2,
    },
  };
}

function serialLogFromFrame(frame: SerialLogFrame): SerialLogEntry {
  return {
    id: `${Date.now().toString(36)}-${Math.random().toString(16).slice(2, 8)}`,
    timestamp: new Date().toISOString(),
    level: frame.level,
    target: frame.target ?? "usb_cdc",
    message: frame.message,
  };
}

function serialTraceFromEvent(entry: SerialTraceEvent): SerialTraceEntry {
  return {
    id: `${Date.now().toString(36)}-${Math.random().toString(16).slice(2, 8)}`,
    timestamp: new Date().toISOString(),
    ...entry,
  };
}

function appendSerialLog(record: DeviceRecord, entry: SerialLogEntry): DeviceRecord {
  if (!record.serial) return record;
  return {
    ...record,
    serial: {
      ...record.serial,
      logs: [...record.serial.logs, entry].slice(-200),
    },
  };
}

function appendSerialTrace(record: DeviceRecord, entry: SerialTraceEntry): DeviceRecord {
  if (!record.serial) return record;
  return {
    ...record,
    serial: {
      ...record.serial,
      trace: [...record.serial.trace, entry],
    },
  };
}

function updateSerialSettings(
  record: DeviceRecord,
  patch: Partial<SafeSettingsState>,
  target: string,
  message: string,
): DeviceRecord {
  if (!record.serial) return record;
  const nextSettings = {
    ...record.serial.safeSettings,
    ...patch,
    manual_charge: patch.manual_charge ?? record.serial.safeSettings.manual_charge,
  };
  return appendSerialLog(
    {
      ...record,
      serial: {
        ...record.serial,
        safeSettings: nextSettings,
      },
      lastUpdated: new Date().toISOString(),
    },
    serialLogFromFrame({ type: "log", level: "info", target, message }),
  );
}

function serialCommandUnavailable(): CommandResult {
  return {
    ok: false,
    error: {
      code: "serial_session_required",
      message: "USB CDC device is not connected in this browser session",
      retryable: true,
      details: null,
    },
  };
}
