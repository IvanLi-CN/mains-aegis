import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  bindDevdCompanionLan,
  clearDeviceWifiConfig,
  clearDevdCompanionLan,
  bridgeAuthToken,
  clearDevdWifiConfig,
  connectDevdDevice,
  createDevdWebLease,
  decodeDefmtFrame,
  disconnectDevdDevice,
  getDeviceChargeControl,
  getDevdDeviceIdentity,
  getDevdDeviceChargeControl,
  getDevdDeviceSettings,
  getDevdDeviceTrace,
  getDevdSerialSession,
  getBridgeBootstrap,
  isTransportErrorEnvelope,
  getSettings,
  heartbeatDevdWebLease,
  getStatus,
  listDevdDevices,
  normalizeBaseUrl,
  previewDeviceChargeControl,
  previewDevdDeviceChargeControl,
  probeDevice,
  releaseDevdWebLease,
  resetDeviceAdvancedPower,
  resetDevdAdvancedPower,
  scanDevdDevices,
  sendDeviceWifiConfig,
  sendDevdWifiConfig,
  setDeviceAdvancedPower,
  setDeviceLogLevel,
  setDeviceManualChargeControl,
  setDevdLogLevel,
  setDevdAdvancedPower,
  setDeviceManualChargePrefs,
  setDevdManualChargeControl,
  setDevdManualChargePrefs,
  subscribeDevdSerialEvents,
  toErrorEnvelope,
  type DevdSerialEventStream,
  type DevdSerialSession,
} from "../api/client";
import { subscribeStatusStream, type StatusStream } from "../api/statusStream";
import {
  buildAdvancedPowerCapabilities,
  buildAdvancedPowerDefaults,
} from "../api/runtimeModeProfiles";
import type {
  AdvancedPowerSettings,
  ActiveAlertsSnapshot,
  ChargeControlDetail,
  DevdDevice,
  DevdWebLease,
  DeviceRecord,
  DeviceSettings,
  DeviceTarget,
  Identity,
  ProbeResult,
  SerialLogEntry,
  SerialTraceEntry,
  UpsStatus,
} from "../api/types";
import {
  isDemoSeed,
  isStoredTargetPreset,
  makeMockRecord,
  makeMockRecords,
  makeStoredTargetPreset,
  makeMockUsbSerialRecord,
  type DemoSeed,
} from "../fixtures/mockDevices";
import { DEFAULT_DEMO_SEED, demoQuerySeed } from "../demo/query";
import {
  findBundledFirmwareArtifact,
  findFirmwareArtifactForIdentity,
  firmwareArtifactElfPath,
  firmwareCatalogSourceLabel,
} from "../firmware/catalog";
import {
  errorFromSerialFailure,
  isWebSerialSupported,
  type SerialHelloFrame,
  type SerialFrame,
  type SerialLogFrame,
  type SerialPortLike,
  type SerialStatusFrame,
  type SerialTraceEvent,
  WebSerialTransport,
} from "../serial/transport";
import {
  DeviceRegistryContext,
  type AddDeviceInput,
  type AddDeviceResult,
  type AdvancedPowerInput,
  type CommandResult,
  type DeviceChannelTransport,
  type ManualChargeControlInput,
  type ManualChargePrefsInput,
  type WifiConfigInput,
  type WifiProvisioningProgress,
} from "./context";

const DEVD_SERIAL_SESSION_LIMITS = {
  logsLimit: 200,
  traceLimit: 600,
};

const STORAGE_KEY = "mains-aegis-web.devices.v1";
const LEGACY_DEVD_TRANSPORT = "ad" + "apter";

export function resolveManualHttpChannelPersistence(input: {
  baseUrl: string;
  rememberedHttpBaseUrl?: string;
  rememberedHttpMdnsHost?: string;
  rememberedHttpFallbackBaseUrl?: string;
  identityHostnameFqdn?: string | null;
  networkIpv4?: string | null;
}) {
  const savedBaseUrl = normalizeBaseUrl(input.baseUrl);
  const rememberedHttpBaseUrl =
    normalizeBaseUrl(
      input.rememberedHttpBaseUrl?.trim() || input.identityHostnameFqdn || "",
    ) || savedBaseUrl;
  const rememberedHttpFallbackBaseUrl =
    normalizeBaseUrl(
      input.rememberedHttpFallbackBaseUrl?.trim() ||
        input.networkIpv4?.trim() ||
        savedBaseUrl,
    ) || undefined;

  return {
    savedBaseUrl,
    rememberedHttpBaseUrl,
    rememberedHttpMdnsHost:
      input.rememberedHttpMdnsHost?.trim() || input.identityHostnameFqdn || "",
    rememberedHttpFallbackBaseUrl:
      rememberedHttpFallbackBaseUrl !== rememberedHttpBaseUrl
        ? rememberedHttpFallbackBaseUrl
        : undefined,
  };
}

export function DeviceRegistryProvider({
  children,
  initialDemoSeed,
}: {
  children: React.ReactNode;
  initialDemoSeed?: DemoSeed;
}) {
  const seedRef = useRef<DemoSeed | null>(getDemoSeed(initialDemoSeed));
  const [demoSeed, setActiveDemoSeed] = useState<DemoSeed | null>(seedRef.current);
  const [records, setRecords] = useState<DeviceRecord[]>(() =>
    loadInitialRecords(seedRef.current),
  );
  const streams = useRef(new Map<string, StatusStream>());
  const devdStreams = useRef(new Map<string, DevdSerialEventStream>());
  const devdLeaseHeartbeats = useRef(new Map<string, number>());
  const serialSessions = useRef(new Map<string, WebSerialTransport>());

  useEffect(() => {
    if (demoSeed) return;
    const targets = records.flatMap(persistedTargetsForRecord);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(targets));
  }, [demoSeed, records]);

  useEffect(() => {
    const syncSeedFromUrl = () => {
      const querySeed = getDemoSeed(initialDemoSeed);
      const nextSeed = querySeed && seedRef.current ? seedRef.current : querySeed;
      if (seedRef.current === nextSeed) return;
      seedRef.current = nextSeed;
      setActiveDemoSeed(nextSeed);
      for (const stream of streams.current.values()) stream.close();
      streams.current.clear();
      for (const stream of devdStreams.current.values()) stream.close();
      devdStreams.current.clear();
      for (const heartbeat of devdLeaseHeartbeats.current.values())
        window.clearInterval(heartbeat);
      devdLeaseHeartbeats.current.clear();
      for (const session of serialSessions.current.values())
        void session.close();
      serialSessions.current.clear();
      if (!nextSeed) {
        setRecords(loadInitialRecords(null));
        return;
      }
      setRecords(makeMockRecords(nextSeed));
    };

    window.addEventListener("popstate", syncSeedFromUrl);
    return () => window.removeEventListener("popstate", syncSeedFromUrl);
  }, [initialDemoSeed]);

  const setRecordError = useCallback(
    (deviceId: string, error: DeviceRecord["error"]) => {
      setRecords((current) =>
        current.map((record) =>
          record.target.deviceId === deviceId
            ? {
                ...record,
                connectionState: error?.retryable ? "offline" : "error",
                streamState: "error",
                error,
                serial: record.serial
                  ? { ...record.serial, connected: false }
                  : record.serial,
                lastUpdated: new Date().toISOString(),
              }
            : record,
        ),
      );
    },
    [],
  );

  const resolveBridgeAuthState = useCallback(
    async (target: Pick<DeviceTarget, "baseUrl" | "bridgeAuth">) => {
      if (target.bridgeAuth) return true;
      return bridgeAuthToken(target.baseUrl) !== null;
    },
    [],
  );

  const setSerialCommandError = useCallback(
    (deviceId: string, error: DeviceRecord["error"]) => {
      if (!serialSessions.current.has(deviceId)) {
        let handledByDevd = false;
        setRecords((current) =>
          current.map((record) => {
            if (record.target.deviceId !== deviceId || !isDevdSerial(record))
              return record;
            handledByDevd = true;
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
                target: "devd",
                message: `${error?.code ?? "devd_error"}: ${error?.message ?? "devd USB command failed"}`,
              }),
            );
          }),
        );
        if (!handledByDevd) {
          const httpCommandFailure = !isTransportErrorEnvelope(error);
          if (httpCommandFailure) {
            setRecords((current) =>
              current.map((record) =>
                record.target.deviceId === deviceId &&
                record.target.transport === "http"
                  ? {
                      ...record,
                      connectionState: "online",
                      streamState:
                        record.streamState === "streaming"
                          ? "streaming"
                          : "polling",
                      error,
                      lastUpdated: new Date().toISOString(),
                    }
                  : record,
              ),
            );
          } else {
            setRecordError(deviceId, error);
          }
        }
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
                  serial: record.serial
                    ? { ...record.serial, connected: true }
                    : record.serial,
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

  const refreshDevice = useCallback(
    async (deviceId: string) => {
      const existing = records.find(
        (record) => record.target.deviceId === deviceId,
      );
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
                    serial: record.serial
                      ? { ...record.serial, connected: true }
                      : record.serial,
                    lastUpdated: new Date().toISOString(),
                  }
                : mergeDeviceRecord(record, makeMockRecord(record.target))
              : record,
          ),
        );
        return;
      }
      const selectedTransport = existing
        ? resolvePreferredTransport(existing, serialSessions.current)
        : (target.transport ?? "http");
      if (selectedTransport === "serial") {
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
                      message:
                        "USB CDC device is not connected in this browser session",
                      retryable: true,
                      details: null,
                    },
                    serial: record.serial
                      ? { ...record.serial, connected: false }
                      : record.serial,
                    lastUpdated: new Date().toISOString(),
                  }
                : record,
            ),
          );
          return;
        }
        setRecords((current) =>
          current.map((record) =>
            record.target.deviceId === deviceId
              ? { ...record, connectionState: "connecting", error: null }
              : record,
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
          record.target.deviceId === deviceId
            ? { ...record, connectionState: "connecting", error: null }
            : record,
        ),
      );

      const cachedBridgeAuth = bridgeAuthToken(target.baseUrl) !== null;
      try {
      if (selectedTransport === "devd") {
        const devdBaseUrl =
          rememberedDevdChannel(existing)?.baseUrl ??
          existing.serial?.baseUrl ??
          target.baseUrl;
        const devdDeviceId =
          rememberedDevdChannel(existing)?.devdDeviceId ?? target.deviceId;
        if (existing.serial?.leaseId) {
          await updateDevdSerialSnapshot(deviceId, devdBaseUrl);
          return;
        }
          const devdTarget = {
            ...target,
            baseUrl: devdBaseUrl,
            transport: "devd" as const,
            preferredTransport: target.preferredTransport,
          };
          const identity = await getDevdDeviceIdentity(
            devdBaseUrl,
            devdDeviceId,
          );
          const settings = await getDevdDeviceSettings(
            devdBaseUrl,
            devdDeviceId,
          );
          const traceSession = await getDevdDeviceTrace(
            devdBaseUrl,
            devdDeviceId,
            DEVD_SERIAL_SESSION_LIMITS,
          );
          const record = recordFromDevdDeviceSnapshot(
            devdTarget,
            identity,
            traceSession.status ?? null,
            settings,
            traceSession,
          );
          setRecords((current) => upsertRecord(current, record));
          return;
        }
        const { nextTarget, result } = await withRememberedHttpFallback(
          existing,
          async (httpBaseUrl) => {
            const httpTarget =
              httpBaseUrl === target.baseUrl
                ? target
                : {
                    ...target,
                    baseUrl: httpBaseUrl,
                    transport: "http" as const,
                  };
            const bridgeAuth = await resolveBridgeAuthState(httpTarget);
            const nextTarget = bridgeAuth
              ? { ...httpTarget, bridgeAuth: true }
              : httpTarget;
            const result = await probeDevice(
              httpTarget.baseUrl,
              undefined,
              bridgeAuth ? { bridgeAuth: true } : undefined,
            );
            return { nextTarget, result };
          },
        );
        setRecords((current) => {
          const previous = current.find(
            (record) => record.target.deviceId === deviceId,
          );
          if (!previous) return current;
          const streamState =
            result.identity.capabilities.sse &&
            previous.streamState !== "polling"
              ? "idle"
              : "polling";
          return upsertRecord(
            current,
            recordFromProbe(nextTarget, result, "online", streamState),
          );
        });
      } catch (error) {
        const envelope = toErrorEnvelope(error);
        setRecords((current) =>
          current.map((record) =>
            record.target.deviceId === deviceId
              ? {
                  ...record,
                  target: {
                    ...record.target,
                    bridgeAuth:
                      record.target.bridgeAuth || cachedBridgeAuth
                        ? true
                        : undefined,
                  },
                  connectionState: envelope.retryable ? "offline" : "error",
                  streamState: "polling",
                  error: envelope,
                  lastUpdated: new Date().toISOString(),
                }
              : record,
          ),
        );
      }
    },
    [records, resolveBridgeAuthState, setRecordError],
  );

  useEffect(() => {
    const interval = window.setInterval(() => {
      for (const record of records) {
        if (
          resolvePreferredTransport(record, serialSessions.current) !==
            "devd" &&
          !streams.current.has(record.target.deviceId)
        ) {
          void refreshDevice(record.target.deviceId);
        }
      }
    }, 10000);
    return () => window.clearInterval(interval);
  }, [records, refreshDevice]);

  useEffect(() => {
    if (demoSeed) return;
    for (const record of records) {
      const devdBaseUrl =
        record.serial?.source === "devd" ? record.serial.baseUrl : null;
      const leaseId = record.serial?.leaseId;
      if (
        !devdBaseUrl ||
        !leaseId ||
        !record.serial?.connected ||
        devdStreams.current.has(record.target.deviceId)
      )
        continue;
      const subscription = subscribeDevdSerialEvents(devdBaseUrl, leaseId, {
        onEvent: (event) => {
          if (event.kind === "serial_trace" && event.payload.trace)
            appendSerialTraceToSession(
              event.payload.trace,
              record.target.deviceId,
            );
          if (event.kind === "serial_log" && event.payload.log)
            appendSerialLogToSession(event.payload.log, record.target.deviceId);
          if (event.kind === "serial_status" && event.payload.status)
            appendSerialStatusToSession(
              event.payload.status,
              record.target.deviceId,
            );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId
                ? {
                    ...candidate,
                    streamState: "streaming",
                    connectionState: "online",
                    error: null,
                    lastUpdated: new Date().toISOString(),
                  }
                : candidate,
            ),
          );
        },
        onError: () => {
          devdStreams.current.get(record.target.deviceId)?.close();
          devdStreams.current.delete(record.target.deviceId);
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId
                ? { ...candidate, streamState: "polling" }
                : candidate,
            ),
          );
          void updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
        },
      });
      devdStreams.current.set(record.target.deviceId, subscription);
    }
    for (const [deviceId, stream] of devdStreams.current.entries()) {
      if (
        !records.some(
          (record) =>
            record.target.deviceId === deviceId &&
            record.serial?.source === "devd" &&
            record.serial.connected,
        )
      ) {
        stream.close();
        devdStreams.current.delete(deviceId);
      }
    }
  }, [demoSeed, records]);

  useEffect(() => {
    for (const record of records) {
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      const httpBaseUrl = rememberedHttpBaseUrl(record);
      if (
        selectedTransport === "serial" ||
        selectedTransport === "devd" ||
        record.target.mock ||
        !record.identity?.capabilities.sse ||
        record.streamState !== "idle" ||
        streams.current.has(record.target.deviceId) ||
        !httpBaseUrl
      ) {
        continue;
      }

      const bridgeAuth = record.target.bridgeAuth
        ? { bridgeAuth: true }
        : undefined;
      const subscription = subscribeStatusStream(
        httpBaseUrl,
        {
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
                  ? {
                      ...candidate,
                      streamState: "streaming",
                      lastUpdated: new Date().toISOString(),
                    }
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
            void withRememberedHttpFallback(record, (baseUrl) =>
              getStatus(baseUrl, undefined, bridgeAuth),
            )
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
                          connectionState: envelope.retryable
                            ? "offline"
                            : "error",
                          streamState: "error",
                          error: envelope,
                          lastUpdated: new Date().toISOString(),
                        }
                      : candidate,
                  ),
                );
              });
          },
        },
        bridgeAuth,
      );

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
    const releaseLeases = () => {
      for (const record of records) releaseDevdLeaseForRecord(record, true);
    };
    window.addEventListener("pagehide", releaseLeases);
    window.addEventListener("beforeunload", releaseLeases);
    return () => {
      window.removeEventListener("pagehide", releaseLeases);
      window.removeEventListener("beforeunload", releaseLeases);
    };
  }, [records]);

  useEffect(() => {
    return () => {
      for (const stream of streams.current.values()) stream.close();
      streams.current.clear();
      for (const stream of devdStreams.current.values()) stream.close();
      devdStreams.current.clear();
      for (const heartbeat of devdLeaseHeartbeats.current.values())
        window.clearInterval(heartbeat);
      devdLeaseHeartbeats.current.clear();
      for (const record of records) releaseDevdLeaseForRecord(record, true);
      for (const session of serialSessions.current.values())
        void session.close();
      serialSessions.current.clear();
    };
  }, []);

  const addDevice = useCallback(
    async (input: AddDeviceInput): Promise<AddDeviceResult> => {
      const baseUrl = normalizeBaseUrl(input.target);

      try {
        const bootstrap = await getBridgeBootstrap(baseUrl);
        if (
          bootstrap?.app?.mode === "http_service" ||
          bootstrap?.app?.mode === "http_service_api_only"
        ) {
          return {
            ok: false,
            error: {
              code: "devd_http_service_requires_devd_panel",
              message:
                "This endpoint is a mains-aegis-devd HTTP service. Connect it from the devd panel, not LAN status.",
              retryable: false,
              details: null,
            },
          };
        }
        const result = await probeDevice(baseUrl);
        const persistedHttpChannel = resolveManualHttpChannelPersistence({
          baseUrl,
          rememberedHttpBaseUrl: input.rememberedHttpBaseUrl,
          rememberedHttpMdnsHost: input.rememberedHttpMdnsHost,
          rememberedHttpFallbackBaseUrl: input.rememberedHttpFallbackBaseUrl,
          identityHostnameFqdn: result.identity.hostname_fqdn,
          networkIpv4: result.network.ipv4,
        });
        const target: DeviceTarget = {
          deviceId: result.identity.device_id,
          baseUrl: persistedHttpChannel.savedBaseUrl,
          alias: input.alias?.trim() || result.identity.hostname,
          location: input.location?.trim() || "Unassigned",
          addedAt: new Date().toISOString(),
          transport: "http",
          preferredTransport: "http",
          rememberedChannels: {
            http: {
              baseUrl: persistedHttpChannel.rememberedHttpBaseUrl,
              seenAt: new Date().toISOString(),
              source: "manual",
              mdnsHost: persistedHttpChannel.rememberedHttpMdnsHost,
              fallbackBaseUrl:
                persistedHttpChannel.rememberedHttpFallbackBaseUrl,
            },
          },
        };
        const record = recordFromProbe(
          target,
          result,
          "online",
          result.identity.capabilities.sse ? "idle" : "polling",
        );
        setRecords((current) => upsertRecord(current, record));
        return { ok: true, record };
      } catch (error) {
        return { ok: false, error: toErrorEnvelope(error) };
      }
    },
    [],
  );

  const addDevdDevice = useCallback(
    async (input: AddDeviceInput): Promise<AddDeviceResult> => {
      const baseUrl = normalizeBaseUrl(input.target);
      let pendingLeaseId: string | null = null;

      try {
        const bridgeAuth = bridgeAuthToken(baseUrl) !== null;
        const scan = await scanDevdDevices(baseUrl);
        const manageableDevices = scan.devices.filter((device) =>
          isManageableDevdDevice(device),
        );
        const selectedDevice = input.devdDeviceId
          ? manageableDevices.find((device) => device.id === input.devdDeviceId)
          : manageableDevices.length === 1
            ? manageableDevices[0]
            : null;
        if (!selectedDevice) {
          return {
            ok: false,
            error: {
              code:
                manageableDevices.length === 0
                  ? "devd_no_manageable_device"
                  : "devd_multiple_devices",
              message:
                manageableDevices.length === 0
                  ? "No USB CDC or LAN device is available through mains-aegis-devd"
                  : "Multiple devices are available; select one device before adding the devd control surface",
              retryable: false,
              details: { devices: manageableDevices },
            },
          };
        }
        if (selectedDevice.transport === "lan") {
          const identity =
            selectedDevice.identity ??
            (await getDevdDeviceIdentity(baseUrl, selectedDevice.id));
          const lanBaseUrl = devdLanBaseUrl(selectedDevice, identity);
          if (!lanBaseUrl) {
            return {
              ok: false,
              error: {
                code: "devd_lan_address_missing",
                message:
                  "This devd LAN device does not expose a direct hardware HTTP target yet",
                retryable: true,
                details: { device: selectedDevice },
              },
            };
          }
          const result = await probeDevice(lanBaseUrl);
          const target: DeviceTarget = {
            deviceId: result.identity.device_id,
            baseUrl: lanBaseUrl,
            alias: input.alias?.trim() || result.identity.hostname,
            location: input.location?.trim() || "LAN",
            addedAt: new Date().toISOString(),
            transport: "http",
            preferredTransport: "http",
            rememberedChannels: {
              http: {
                baseUrl: lanBaseUrl,
                seenAt: new Date().toISOString(),
                source: "devd_discovery",
              },
            },
          };
          const record = recordFromProbe(
            target,
            result,
            "online",
            result.identity.capabilities.sse ? "idle" : "polling",
          );
          setRecords((current) => upsertRecord(current, record));
          return { ok: true, record };
        }
        const lease = await createDevdWebLease(baseUrl, selectedDevice.id);
        pendingLeaseId = lease.lease_id;
        const result = await probeDevice(
          baseUrl,
          lease.lease_id,
          bridgeAuth ? { bridgeAuth: true } : undefined,
        );
        const firmwareMatch = await findFirmwareArtifactForIdentity(
          result.identity,
        );
        if (!firmwareMatch && !input.ignoreFirmwareMismatch) {
          await releaseDevdWebLease(baseUrl, lease.lease_id).catch(
            () => undefined,
          );
          pendingLeaseId = null;
          return {
            ok: false,
            error: firmwareMismatchError(result.identity),
          };
        }
        const session = await getDevdSerialSession(baseUrl, {
          ...DEVD_SERIAL_SESSION_LIMITS,
          leaseId: lease.lease_id,
        });
        const target: DeviceTarget = {
          deviceId: result.identity.device_id,
          baseUrl,
          alias: input.alias?.trim() || result.identity.hostname,
          location: input.location?.trim() || "devd",
          addedAt: new Date().toISOString(),
          preferredTransport: "devd",
          rememberedChannels: {
            devd: {
              baseUrl,
              devdDeviceId: selectedDevice.id,
              seenAt: new Date().toISOString(),
              transport: selectedDevice.transport === "mock" ? "mock" : "usb",
            },
          },
          bridgeAuth: bridgeAuth || undefined,
          transport: "devd",
          serialProtocol: session.protocol,
        };
        const record = recordFromDevdProbe(target, result, session, lease);
        startDevdLeaseHeartbeat(record);
        pendingLeaseId = null;
        setRecords((current) => upsertRecord(current, record));
        return { ok: true, record };
      } catch (error) {
        if (pendingLeaseId)
          await releaseDevdWebLease(baseUrl, pendingLeaseId).catch(
            () => undefined,
          );
        return { ok: false, error: toErrorEnvelope(error) };
      }
    },
    [],
  );

  const confirmDevdCompanionLan = useCallback(
    async (deviceId: string, devdBaseUrl: string): Promise<AddDeviceResult> => {
      try {
        const updated = await bindDevdCompanionLan(deviceId, {}, devdBaseUrl);
        const companion = updated.binding?.lan_companion;
        if (!companion) {
          return {
            ok: false,
            error: {
              code: "companion_lan_bind_failed",
              message: "devd did not return a saved LAN companion binding",
              retryable: true,
              details: { deviceId },
            },
          };
        }
        const companionIpBaseUrl = companion.ip.startsWith("mock:")
          ? companion.ip
          : `http://${companion.ip}:${companion.port}`;
        const companionMdnsBaseUrl = companion.mdns_host.startsWith("mock:")
          ? companion.mdns_host
          : `http://${companion.mdns_host}`;
        const fallbackBaseUrl = normalizeBaseUrl(companionIpBaseUrl);
        const mdnsBaseUrl = normalizeBaseUrl(companionMdnsBaseUrl);
        const preferredBaseUrl = mdnsBaseUrl;
        let successfulBaseUrl = preferredBaseUrl;
        let result: ProbeResult;
        try {
          result = await probeDevice(preferredBaseUrl);
        } catch (error) {
          if (!fallbackBaseUrl || fallbackBaseUrl === preferredBaseUrl)
            throw error;
          successfulBaseUrl = fallbackBaseUrl;
          result = await probeDevice(fallbackBaseUrl);
        }
        const logicalDeviceId =
          updated.binding?.logical_device_id ?? result.identity.device_id;
        const target: DeviceTarget = {
          deviceId: logicalDeviceId,
          baseUrl: successfulBaseUrl,
          alias: result.identity.hostname,
          location: "LAN",
          addedAt: new Date().toISOString(),
          transport: "http",
          preferredTransport: "http",
          rememberedChannels: {
            http: {
              baseUrl: preferredBaseUrl,
              seenAt: new Date().toISOString(),
              source: "devd_discovery",
              mdnsHost: companion.mdns_host,
              fallbackBaseUrl,
            },
            devd: {
              baseUrl: devdBaseUrl,
              devdDeviceId: updated.id,
              seenAt: new Date().toISOString(),
              transport: updated.transport === "mock" ? "mock" : "usb",
            },
          },
        };
        const record = recordFromProbe(
          target,
          result,
          "online",
          result.identity.capabilities.sse ? "idle" : "polling",
        );
        let mergedRecord = record;
        setRecords((current) => {
          const existing = current.find(
            (candidate) => candidate.target.deviceId === logicalDeviceId,
          );
          const nextTarget: DeviceTarget = existing
            ? {
                ...target,
                alias: existing.target.alias || target.alias,
                location: existing.target.location || target.location,
                addedAt: existing.target.addedAt || target.addedAt,
              }
            : target;
          const nextRecord = recordFromProbe(
            nextTarget,
            result,
            "online",
            result.identity.capabilities.sse ? "idle" : "polling",
          );
          const merged = upsertRecord(current, nextRecord);
          mergedRecord =
            merged.find(
              (candidate) => candidate.target.deviceId === logicalDeviceId,
            ) ?? nextRecord;
          return merged;
        });
        return { ok: true, record: mergedRecord };
      } catch (error) {
        return { ok: false, error: toErrorEnvelope(error) };
      }
    },
    [],
  );

  const dismissDevdCompanionLan = useCallback(
    async (deviceId: string, devdBaseUrl: string): Promise<AddDeviceResult> => {
      try {
        const updated = await clearDevdCompanionLan(deviceId, devdBaseUrl);
        const logicalDeviceId =
          updated.binding?.logical_device_id ?? updated.identity?.device_id;
        const existing =
          logicalDeviceId === null
            ? null
            : records.find(
                (record) => record.target.deviceId === logicalDeviceId,
              ) ?? null;
        if (existing) return { ok: true, record: existing };
        return {
          ok: true,
          record: recordFromStoredTarget({
            deviceId: logicalDeviceId ?? deviceId,
            baseUrl: normalizeBaseUrl(devdBaseUrl),
            alias: updated.display_name || "Saved device",
            location: "USB",
            addedAt: new Date().toISOString(),
            transport: "devd",
          }),
        };
      } catch (error) {
        return { ok: false, error: toErrorEnvelope(error) };
      }
    },
    [records],
  );

  const connectUsbSerialDevice = useCallback(
    async (
      input: Pick<
        AddDeviceInput,
        "alias" | "location" | "ignoreFirmwareMismatch"
      > = {},
    ): Promise<AddDeviceResult> => {
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
            const deviceId = transportRef
              ? findSessionDeviceId(transportRef)
              : null;
            if (!deviceId) {
              if (frame.type === "log")
                pendingLogs.push(serialLogFromFrame(frame));
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
            const deviceId = transportRef
              ? findSessionDeviceId(transportRef)
              : null;
            if (!deviceId) {
              pendingTrace.push(serialTraceFromEvent(entry));
              return;
            }
            appendSerialTraceToSession(entry, deviceId);
          },
          onDefmtLog: (decoded) => {
            const log = serialLogFromFrame({
              type: "log",
              level: decoded.level,
              target: decoded.target,
              message: decoded.message,
            });
            const deviceId = transportRef
              ? findSessionDeviceId(transportRef)
              : null;
            if (!deviceId) {
              pendingLogs.push(log);
              return;
            }
            appendSerialLogToSession(log, deviceId);
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
        const settings = await loadUsbProbeSettings(hello, transport);
        const identity = hello.identity;
        const firmwareMatch = await findFirmwareArtifactForIdentity(identity);
        if (!firmwareMatch && !input.ignoreFirmwareMismatch) {
          await transport.close().catch(() => undefined);
          openedTransport = null;
          return {
            ok: false,
            error: firmwareMismatchError(identity),
          };
        }
        const target: DeviceTarget = {
          deviceId: identity.device_id,
          baseUrl: `serial:${identity.device_id}`,
          alias: input.alias?.trim() || identity.hostname,
          location: input.location?.trim() || "USB",
          addedAt: new Date().toISOString(),
          transport: "serial",
          preferredTransport: "serial",
          rememberedChannels: {
            serial: {
              seenAt: new Date().toISOString(),
            },
          },
          serialProtocol: hello.protocol,
        };
        serialSessions.current.set(identity.device_id, transport);
        openedTransport = null;
        const decoderArtifact =
          firmwareMatch?.source === "github_release"
            ? await findBundledFirmwareArtifact(identity)
            : firmwareMatch?.artifact;
        const bundledElfPath = decoderArtifact
          ? firmwareArtifactElfPath(decoderArtifact)
          : null;
        transport.setDefmtDecoder(
          bundledElfPath
            ? (frame) =>
                decodeDefmtFrame({
                  elf_path: bundledElfPath,
                  frame_hex: bytesToHex(frame),
                })
            : null,
        );
        const record = recordFromSerialProbe(
          target,
          {
            identity,
            network: identity.network,
            status,
            settings,
          },
          hello.protocol,
          [
            ...pendingLogs,
            serialLogFromFrame(
              firmwareMatch
                ? {
                    type: "log",
                    level: "info",
                    target: "firmware_catalog",
                    message: `${firmwareCatalogSourceLabel(firmwareMatch.source)} firmware artifact matched: ${firmwareMatch.artifact.artifact_id}`,
                  }
                : {
                    type: "log",
                    level: "warn",
                    target: "firmware_catalog",
                    message: `No GitHub Release or bundled firmware artifact matches build ${identity.firmware.build_id}; defmt binary remains undecoded`,
                  },
            ),
            ...(firmwareMatch?.source === "github_release"
              ? [
                  serialLogFromFrame({
                    type: "log",
                    level: bundledElfPath ? "info" : "warn",
                    target: "firmware_catalog",
                    message: bundledElfPath
                      ? `GitHub Release firmware artifact matched: ${firmwareMatch.artifact.artifact_id}; using bundled ELF for defmt decode`
                      : `GitHub Release firmware artifact matched: ${firmwareMatch.artifact.artifact_id}; defmt decode artifact is unavailable locally`,
                  }),
                ]
              : []),
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

  const stageDeviceRecord = useCallback((record: DeviceRecord) => {
    setRecords((current) => upsertRecord(current, record));
  }, []);

  const rememberDiscoveredChannels = useCallback(
    (devdBaseUrl: string, devices: DevdDevice[]) => {
      const discoveryByDeviceId = new Map<
        string,
        Partial<NonNullable<DeviceTarget["rememberedChannels"]>>
      >();
      for (const device of devices) {
        const logicalDeviceId = devdLogicalDeviceId(device);
        if (!logicalDeviceId) continue;
        const current = discoveryByDeviceId.get(logicalDeviceId) ?? {};
        if (device.lan_address) {
          const lanBaseUrl = devdLanBaseUrl(device, device.identity);
          if (!lanBaseUrl) continue;
          current.http = {
            baseUrl: lanBaseUrl,
            seenAt: new Date().toISOString(),
            source: "devd_discovery",
          };
        }
        if (device.transport !== "lan") {
          current.devd = {
            baseUrl: devdBaseUrl,
            devdDeviceId: device.id,
            seenAt: new Date().toISOString(),
            transport: device.transport === "mock" ? "mock" : "usb",
          };
        }
        discoveryByDeviceId.set(logicalDeviceId, current);
      }
      if (discoveryByDeviceId.size === 0) return;
      setRecords((current) =>
        current.map((record) => {
          const memory = discoveryByDeviceId.get(record.target.deviceId);
          if (!memory) return record;
          return {
            ...record,
            target: {
              ...record.target,
              rememberedChannels: mergeRememberedChannels(
                record.target.rememberedChannels,
                memory,
              ),
            },
          };
        }),
      );
    },
    [],
  );

  const connectKnownDeviceChannel = useCallback(
    async (
      deviceId: string,
      transport: DeviceChannelTransport,
      options: Pick<AddDeviceInput, "ignoreFirmwareMismatch"> = {},
    ): Promise<AddDeviceResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) {
        return {
          ok: false,
          error: {
            code: "device_not_found",
            message: "The selected device is no longer in the local registry",
            retryable: false,
            details: { deviceId },
          },
        };
      }

      if (transport === "http") {
        const baseUrls = rememberedHttpBaseUrls(record);
        if (baseUrls.length === 0) return unavailableChannelError("http");
        let lastResult: AddDeviceResult | null = null;
        for (const baseUrl of baseUrls) {
          const result = await addDevice({
            target: baseUrl,
            alias: record.target.alias,
            location: record.target.location,
          });
          if (result.ok) return result;
          lastResult = result;
        }
        return lastResult ?? unavailableChannelError("http");
      }

      if (transport === "devd") {
        const devdChannel = rememberedDevdChannel(record);
        if (!devdChannel || devdChannel.baseUrl === undefined)
          return unavailableChannelError("devd");
        let devdDeviceId = devdChannel.devdDeviceId ?? null;
        if (!devdDeviceId) {
          const scan = await scanDevdDevices(devdChannel.baseUrl);
          const matches = scan.devices.filter(
            (device) =>
              isManageableDevdDevice(device) &&
              devdLogicalDeviceId(device) === record.target.deviceId,
          );
          if (matches.length !== 1) return unavailableChannelError("devd");
          devdDeviceId = matches[0].id;
        }
        return addDevdDevice({
          target: devdChannel.baseUrl,
          devdDeviceId,
          alias: record.target.alias,
          location: record.target.location,
          ignoreFirmwareMismatch: options.ignoreFirmwareMismatch,
        });
      }

      return connectUsbSerialDevice({
        alias: record.target.alias,
        location: record.target.location,
        ignoreFirmwareMismatch: options.ignoreFirmwareMismatch,
      });
    },
    [addDevice, addDevdDevice, connectUsbSerialDevice, records],
  );

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
              serial: record.serial
                ? { ...record.serial, connected: false }
                : record.serial,
              lastUpdated: new Date().toISOString(),
            }
          : record,
      ),
    );
  }, []);

  const prepareWebSerialFlashPort = useCallback(
    async (deviceId: string): Promise<SerialPortLike | null> => {
      const session = serialSessions.current.get(deviceId);
      if (!session) return null;
      serialSessions.current.delete(deviceId);
      return session.releasePort();
    },
    [],
  );

  const sendWifiConfig = useCallback(
    async (
      deviceId: string,
      input: WifiConfigInput,
      onProgress?: (progress: WifiProvisioningProgress) => void,
    ): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      if (record.target.mock) {
        onProgress?.({
          phase: "connected",
          message: `WiFi connected to ${input.ssid} at 192.168.31.42`,
          network: {
            state: "connected",
            ipv4: "192.168.31.42",
            last_error: null,
          },
        });
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? updateSerialSettings(
                  candidate,
                  {
                    wifi_configured: true,
                    wifi_ssid: input.ssid,
                  },
                  "wifi_config",
                  `WiFi credentials saved for ${input.ssid}`,
                )
              : candidate,
          ),
        );
        return { ok: true };
      }
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      if (selectedTransport === "http") {
        try {
          onProgress?.({
            phase: "saving",
            message: "Writing WiFi credentials over LAN",
          });
          const { status, settings } = await withRememberedHttpFallback(
            record,
            async (httpBaseUrl) => {
              await sendDeviceWifiConfig(httpBaseUrl, input);
              onProgress?.({
                phase: "connecting",
                message: `Connecting to ${input.ssid} and waiting for an IP address`,
              });
              const status = await waitForHttpWifiConnected(
                httpBaseUrl,
                input.ssid,
                onProgress,
              );
              const settings = await getSettings(httpBaseUrl);
              return { status, settings };
            },
          );
          const message = wifiConnectedMessage(input.ssid, status.network);
          onProgress?.({
            phase: status.network.ipv4 ? "connected" : "ip",
            message,
            network: status.network,
          });
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? mergeLanDeviceSnapshot(candidate, status, settings, message)
                : candidate,
            ),
          );
          return { ok: true, message, network: status.network };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope };
        }
      }
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const leaseId = devdLeaseIdForRecord(record);
          onProgress?.({
            phase: "saving",
            message: "Writing WiFi credentials to hardware",
          });
          onProgress?.({
            phase: "connecting",
            message: `Connecting to ${input.ssid} and waiting for an IP address`,
          });
          const applyResult = await sendDevdWifiConfig(
            devdBaseUrl,
            devdDeviceId,
            leaseId,
            input,
          );
          const settings = leaseId
            ? null
            : await getDevdDeviceSettings(devdBaseUrl, devdDeviceId);
          if (leaseId)
            await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
          const message = wifiConnectedMessage(input.ssid, applyResult.network);
          onProgress?.({
            phase: applyResult.network.ipv4 ? "connected" : "ip",
            message,
            network: applyResult.network,
          });
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? settings
                  ? mergeLanDeviceSnapshot(
                      candidate,
                      undefined,
                      settings,
                      message,
                    )
                  : updateSerialSettings(
                      candidate,
                      {
                        wifi_configured: true,
                        wifi_ssid: input.ssid,
                      },
                      "wifi_config",
                      message,
                    )
                : candidate,
            ),
          );
          return { ok: true, message, network: applyResult.network };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope };
        }
      }
      const session = serialSessions.current.get(deviceId);
      if (!session) return serialCommandUnavailable();
      try {
        onProgress?.({
          phase: "saving",
          message: "Writing WiFi credentials to hardware",
        });
        await session.setWifiConfig(input.ssid, input.psk);
        onProgress?.({
          phase: "connecting",
          message: `Connecting to ${input.ssid} and waiting for an IP address`,
        });
        const status = await waitForSerialWifiConnected(
          session,
          input.ssid,
          onProgress,
        );
        const message = wifiConnectedMessage(input.ssid, status.network);
        onProgress?.({
          phase: status.network.ipv4 ? "connected" : "ip",
          message,
          network: status.network,
        });
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? updateSerialSettings(
                  candidate,
                  {
                    wifi_configured: true,
                    wifi_ssid: input.ssid,
                  },
                  "wifi_config",
                  message,
                )
              : candidate,
          ),
        );
        return { ok: true, message, network: status.network };
      } catch (error) {
        const envelope = errorFromSerialFailure(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    },
    [records, setSerialCommandError],
  );

  const clearWifiConfig = useCallback(
    async (
      deviceId: string,
      onProgress?: (progress: WifiProvisioningProgress) => void,
    ): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      if (record.target.mock) {
        onProgress?.({
          phase: "disabled",
          message: "WiFi credentials cleared and WiFi disconnected",
          network: { state: "disabled", ipv4: null, last_error: null },
        });
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? updateSerialSettings(
                  candidate,
                  { wifi_configured: false, wifi_ssid: null },
                  "wifi_config",
                  "WiFi credentials cleared",
                )
              : candidate,
          ),
        );
        return { ok: true };
      }
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      if (selectedTransport === "http") {
        try {
          onProgress?.({
            phase: "clearing",
            message: "Clearing WiFi credentials over LAN",
          });
          const { status, settings } = await withRememberedHttpFallback(
            record,
            async (httpBaseUrl) => {
              await clearDeviceWifiConfig(httpBaseUrl);
              const status = await waitForHttpWifiDisabled(
                httpBaseUrl,
                onProgress,
              );
              const settings = await getSettings(httpBaseUrl);
              return { status, settings };
            },
          );
          const message = wifiDisabledMessage(status.network);
          onProgress?.({ phase: "disabled", message, network: status.network });
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? mergeLanDeviceSnapshot(candidate, status, settings, message)
                : candidate,
            ),
          );
          return { ok: true, message, network: status.network };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope };
        }
      }
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const leaseId = devdLeaseIdForRecord(record);
          onProgress?.({
            phase: "clearing",
            message: "Clearing WiFi credentials from hardware",
          });
          const applyResult = await clearDevdWifiConfig(
            devdBaseUrl,
            devdDeviceId,
            leaseId,
          );
          const settings = leaseId
            ? null
            : await getDevdDeviceSettings(devdBaseUrl, devdDeviceId);
          if (leaseId)
            await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
          const message = wifiDisabledMessage(applyResult.network);
          onProgress?.({
            phase: "disabled",
            message,
            network: applyResult.network,
          });
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? settings
                  ? mergeLanDeviceSnapshot(
                      candidate,
                      undefined,
                      settings,
                      message,
                    )
                  : updateSerialSettings(
                      candidate,
                      { wifi_configured: false, wifi_ssid: null },
                      "wifi_config",
                      message,
                    )
                : candidate,
            ),
          );
          return { ok: true, message, network: applyResult.network };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope };
        }
      }
      const session = serialSessions.current.get(deviceId);
      if (!session) return serialCommandUnavailable();
      try {
        onProgress?.({
          phase: "clearing",
          message: "Clearing WiFi credentials from hardware",
        });
        await session.clearWifiConfig();
        const status = await waitForSerialWifiDisabled(session, onProgress);
        const message = wifiDisabledMessage(status.network);
        onProgress?.({ phase: "disabled", message, network: status.network });
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? updateSerialSettings(
                  candidate,
                  { wifi_configured: false, wifi_ssid: null },
                  "wifi_config",
                  message,
                )
              : candidate,
          ),
        );
        return { ok: true, message, network: status.network };
      } catch (error) {
        const envelope = errorFromSerialFailure(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    },
    [records, setSerialCommandError],
  );

  const setSerialLogLevel = useCallback(
    async (
      deviceId: string,
      level: DeviceSettings["log_level"],
    ): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      if (selectedTransport === "http") {
        try {
          const settings = await withRememberedHttpFallback(
            record,
            async (httpBaseUrl) => {
              await setDeviceLogLevel(httpBaseUrl, level);
              return getSettings(httpBaseUrl);
            },
          );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? mergeLanDeviceSnapshot(
                    candidate,
                    undefined,
                    settings,
                    `Log level set to ${level}`,
                  )
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
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const leaseId = devdLeaseIdForRecord(record);
          await setDevdLogLevel(
            devdBaseUrl,
            devdDeviceId,
            leaseId,
            level,
          );
          if (leaseId) {
            await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
          } else {
            const settings = await getDevdDeviceSettings(
              devdBaseUrl,
              devdDeviceId,
            );
            setRecords((current) =>
              current.map((candidate) =>
                candidate.target.deviceId === deviceId
                  ? mergeLanDeviceSnapshot(
                      candidate,
                      undefined,
                      settings,
                      `Log level set to ${level}`,
                    )
                  : candidate,
              ),
            );
            return { ok: true };
          }
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
            ? updateSerialSettings(
                candidate,
                { log_level: level },
                "usb_cdc",
                `Log level set to ${level}`,
              )
            : candidate,
        ),
      );
      return { ok: true };
    },
    [records, setSerialCommandError],
  );

  const setManualChargePrefs = useCallback(
    async (
      deviceId: string,
      prefs: ManualChargePrefsInput,
    ): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      if (selectedTransport === "http") {
        try {
          const settings = await withRememberedHttpFallback(
            record,
            async (httpBaseUrl) => {
              await setDeviceManualChargePrefs(httpBaseUrl, prefs);
              return getSettings(httpBaseUrl);
            },
          );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? mergeLanDeviceSnapshot(
                    candidate,
                    undefined,
                    settings,
                    "Manual charge preferences updated",
                  )
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
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const leaseId = devdLeaseIdForRecord(record);
          await setDevdManualChargePrefs(
            devdBaseUrl,
            devdDeviceId,
            leaseId,
            prefs,
          );
          if (leaseId) {
            await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
          } else {
            const settings = await getDevdDeviceSettings(
              devdBaseUrl,
              devdDeviceId,
            );
            setRecords((current) =>
              current.map((candidate) =>
                candidate.target.deviceId === deviceId
                  ? mergeLanDeviceSnapshot(
                      candidate,
                      undefined,
                      settings,
                      "Manual charge preferences updated",
                    )
                  : candidate,
              ),
            );
            return { ok: true };
          }
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
            ? updateSerialSettings(
                candidate,
                { manual_charge: prefs },
                "manual_charge",
                "Manual charge preferences updated",
              )
            : candidate,
        ),
      );
      return { ok: true };
    },
    [records, setSerialCommandError],
  );

  const refreshChargeControlDetail = useCallback(
    async (deviceId: string): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      if (record.target.mock) {
        const detail = await getDeviceChargeControl(record.target.baseUrl);
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? patchSerialStatusRecord(
                  candidate,
                  chargeControlPatchFromDetail(detail),
                  "manual_charge",
                  "Charge control detail refreshed",
                )
              : candidate,
          ),
        );
        return { ok: true, detail };
      }
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      if (selectedTransport === "http") {
        try {
          const detail = await withRememberedHttpFallback(record, (httpBaseUrl) =>
            getDeviceChargeControl(httpBaseUrl),
          );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? patchSerialStatusRecord(
                    candidate,
                    chargeControlPatchFromDetail(detail),
                    "manual_charge",
                    "Charge control detail refreshed",
                  )
                : candidate,
            ),
          );
          return { ok: true, detail };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return {
            ok: false,
            error: envelope,
            detail: chargeControlDetailFromErrorDetails(envelope.details),
          };
        }
      }
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const detail = await getDevdDeviceChargeControl(
            devdBaseUrl,
            devdDeviceId,
          );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? patchSerialStatusRecord(
                    candidate,
                    chargeControlPatchFromDetail(detail),
                    "manual_charge",
                    "Charge control detail refreshed",
                  )
                : candidate,
            ),
          );
          return { ok: true, detail };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return {
            ok: false,
            error: envelope,
            detail: chargeControlDetailFromErrorDetails(envelope.details),
          };
        }
      }
      const session = serialSessions.current.get(deviceId);
      if (!session) return serialCommandUnavailable();
      try {
        const detail = await session.requestChargeControl();
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? patchSerialStatusRecord(
                  candidate,
                  chargeControlPatchFromDetail(detail),
                  "manual_charge",
                  "Charge control detail refreshed",
                )
              : candidate,
          ),
        );
        return { ok: true, detail };
      } catch (error) {
        const envelope = errorFromSerialFailure(error);
        setSerialCommandError(deviceId, envelope);
        return {
          ok: false,
          error: envelope,
          detail: chargeControlDetailFromErrorDetails(envelope.details),
        };
      }
    },
    [records, setSerialCommandError],
  );

  const previewManualCharge = useCallback(
    async (
      deviceId: string,
      prefs: ManualChargePrefsInput,
    ): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      const input = manualChargePreviewInput(prefs);
      if (record.target.mock) {
        const detail = await previewDeviceChargeControl(
          record.target.baseUrl,
          input,
        );
        return { ok: true, detail };
      }
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      if (selectedTransport === "http") {
        try {
          const detail = await withRememberedHttpFallback(record, (httpBaseUrl) =>
            previewDeviceChargeControl(httpBaseUrl, input),
          );
          return { ok: true, detail };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return {
            ok: false,
            error: envelope,
            detail: chargeControlDetailFromErrorDetails(envelope.details),
          };
        }
      }
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const detail = await previewDevdDeviceChargeControl(
            devdBaseUrl,
            devdDeviceId,
            devdLeaseIdForRecord(record),
            input,
          );
          return { ok: true, detail };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return {
            ok: false,
            error: envelope,
            detail: chargeControlDetailFromErrorDetails(envelope.details),
          };
        }
      }
      const session = serialSessions.current.get(deviceId);
      if (!session) return serialCommandUnavailable();
      try {
        const detail = await session.previewChargeControl(input);
        return { ok: true, detail };
      } catch (error) {
        const envelope = errorFromSerialFailure(error);
        setSerialCommandError(deviceId, envelope);
        return {
          ok: false,
          error: envelope,
          detail: chargeControlDetailFromErrorDetails(envelope.details),
        };
      }
    },
    [records, setSerialCommandError],
  );

  const controlManualCharge = useCallback(
    async (
      deviceId: string,
      input: ManualChargeControlInput,
    ): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      const successMessage =
        input.action === "start"
          ? input.confirm_loop
            ? "Manual charge started with USB-C loop override"
            : "Manual charge started"
          : "Manual charge stopped";
      if (selectedTransport === "http") {
        try {
          const detail = await withRememberedHttpFallback(
            record,
            (httpBaseUrl) => setDeviceManualChargeControl(httpBaseUrl, input),
          );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? patchSerialStatusRecord(
                    candidate,
                    chargeControlPatchFromDetail(detail),
                    "manual_charge",
                    successMessage,
                  )
                : candidate,
            ),
          );
          return { ok: true, message: successMessage, detail };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          const detail = chargeControlDetailFromErrorDetails(envelope.details);
          if (detail) {
            setRecords((current) =>
              current.map((candidate) =>
                candidate.target.deviceId === deviceId
                  ? patchSerialStatusRecord(
                      candidate,
                      chargeControlPatchFromDetail(detail),
                      "manual_charge",
                      "Charge control detail updated from action failure",
                    )
                  : candidate,
              ),
            );
          }
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope, detail };
        }
      }
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const leaseId = devdLeaseIdForRecord(record);
          const detail = await setDevdManualChargeControl(
            devdBaseUrl,
            devdDeviceId,
            leaseId,
            input,
          );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? patchSerialStatusRecord(
                    candidate,
                    chargeControlPatchFromDetail(detail),
                    "manual_charge",
                    successMessage,
                  )
                : candidate,
            ),
          );
          return { ok: true, message: successMessage, detail };
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          const detail = chargeControlDetailFromErrorDetails(envelope.details);
          if (detail) {
            setRecords((current) =>
              current.map((candidate) =>
                candidate.target.deviceId === deviceId
                  ? patchSerialStatusRecord(
                      candidate,
                      chargeControlPatchFromDetail(detail),
                      "manual_charge",
                      "Charge control detail updated from action failure",
                    )
                  : candidate,
              ),
            );
          }
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope, detail };
        }
      }
      if (!record.target.mock) {
        const session = serialSessions.current.get(deviceId);
        if (!session) return serialCommandUnavailable();
        try {
          const detail = await session.controlManualCharge(input);
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? patchSerialStatusRecord(
                    candidate,
                    chargeControlPatchFromDetail(detail),
                    "manual_charge",
                    successMessage,
                  )
                : candidate,
            ),
          );
          return { ok: true, message: successMessage, detail };
        } catch (error) {
          const envelope = errorFromSerialFailure(error);
          const detail = chargeControlDetailFromErrorDetails(envelope.details);
          if (detail) {
            setRecords((current) =>
              current.map((candidate) =>
                candidate.target.deviceId === deviceId
                  ? patchSerialStatusRecord(
                      candidate,
                      chargeControlPatchFromDetail(detail),
                      "manual_charge",
                      "Charge control detail updated from action failure",
                    )
                  : candidate,
              ),
            );
          }
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope, detail };
        }
      }
      const detail = await getDeviceChargeControl(record.target.baseUrl);
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? patchSerialStatusRecord(
                candidate,
                chargeControlPatchFromDetail(detail),
                "manual_charge",
                successMessage,
              )
            : candidate,
        ),
      );
      return { ok: true, message: successMessage, detail };
    },
    [records, setSerialCommandError],
  );

  const setAdvancedPower = useCallback(
    async (
      deviceId: string,
      advancedPower: AdvancedPowerInput,
    ): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      if (selectedTransport === "http") {
        try {
          const settings = await withRememberedHttpFallback(
            record,
            async (httpBaseUrl) => {
              await setDeviceAdvancedPower(httpBaseUrl, advancedPower);
              return getSettings(httpBaseUrl);
            },
          );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? mergeLanDeviceSnapshot(
                    candidate,
                    undefined,
                    settings,
                    "Advanced power settings updated",
                  )
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
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const leaseId = devdLeaseIdForRecord(record);
          await setDevdAdvancedPower(
            devdBaseUrl,
            devdDeviceId,
            leaseId,
            advancedPower,
          );
          if (leaseId) {
            await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
            return { ok: true };
          } else {
            const settings = await getDevdDeviceSettings(
              devdBaseUrl,
              devdDeviceId,
            );
            setRecords((current) =>
              current.map((candidate) =>
                candidate.target.deviceId === deviceId
                  ? mergeLanDeviceSnapshot(
                      candidate,
                      undefined,
                      settings,
                      "Advanced power settings updated",
                    )
                  : candidate,
              ),
            );
            return { ok: true };
          }
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope };
        }
      } else if (!record.target.mock) {
        const session = serialSessions.current.get(deviceId);
        if (!session) return serialCommandUnavailable();
        try {
          await session.setAdvancedPower(advancedPower);
          const settings = await session.requestSettings();
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? updateSerialSettings(
                    candidate,
                    { advanced_power: settings.advanced_power, advanced_power_capabilities: settings.advanced_power_capabilities },
                    "usb_cdc",
                    "Advanced power settings updated",
                  )
                : candidate,
            ),
          );
          return { ok: true };
        } catch (error) {
          const envelope = errorFromSerialFailure(error);
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope };
        }
      }
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(
                candidate,
                { advanced_power: advancedPower },
                "mock",
                "Advanced power settings updated",
              )
            : candidate,
        ),
      );
      return { ok: true };
    },
    [records, setSerialCommandError],
  );

  const resetAdvancedPower = useCallback(
    async (deviceId: string): Promise<CommandResult> => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      if (!record) return serialCommandUnavailable();
      const selectedTransport = resolvePreferredTransport(
        record,
        serialSessions.current,
      );
      if (selectedTransport === "http") {
        try {
          const settings = await withRememberedHttpFallback(
            record,
            async (httpBaseUrl) => {
              await resetDeviceAdvancedPower(httpBaseUrl);
              return getSettings(httpBaseUrl);
            },
          );
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? mergeLanDeviceSnapshot(
                    candidate,
                    undefined,
                    settings,
                    "Advanced power settings reset",
                  )
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
      if (selectedTransport === "devd") {
        const devdBaseUrl = devdBaseUrlForRecord(record);
        if (devdBaseUrl === null) return unavailableCommandChannel("devd");
        const devdDeviceId = devdDeviceIdForRecord(record) ?? record.target.deviceId;
        try {
          const leaseId = devdLeaseIdForRecord(record);
          await resetDevdAdvancedPower(
            devdBaseUrl,
            devdDeviceId,
            leaseId,
          );
          if (leaseId) {
            await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
            return { ok: true };
          } else {
            const settings = await getDevdDeviceSettings(
              devdBaseUrl,
              devdDeviceId,
            );
            setRecords((current) =>
              current.map((candidate) =>
                candidate.target.deviceId === deviceId
                  ? mergeLanDeviceSnapshot(
                      candidate,
                      undefined,
                      settings,
                      "Advanced power settings reset",
                    )
                  : candidate,
              ),
            );
            return { ok: true };
          }
        } catch (error) {
          const envelope = toErrorEnvelope(error);
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope };
        }
      } else if (!record.target.mock) {
        const session = serialSessions.current.get(deviceId);
        if (!session) return serialCommandUnavailable();
        try {
          await session.resetAdvancedPower();
          const settings = await session.requestSettings();
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === deviceId
                ? updateSerialSettings(
                    candidate,
                    { advanced_power: settings.advanced_power, advanced_power_capabilities: settings.advanced_power_capabilities },
                    "usb_cdc",
                    "Advanced power settings reset",
                  )
                : candidate,
            ),
          );
          return { ok: true };
        } catch (error) {
          const envelope = errorFromSerialFailure(error);
          setSerialCommandError(deviceId, envelope);
          return { ok: false, error: envelope };
        }
      }
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(
                candidate,
                {
                  advanced_power: defaultDeviceSettings().advanced_power,
                  advanced_power_capabilities:
                    defaultDeviceSettings().advanced_power_capabilities,
                },
                "mock",
                "Advanced power settings reset",
              )
            : candidate,
        ),
      );
      return { ok: true };
    },
    [records, setSerialCommandError],
  );

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

  function updateSerialStatus(
    frame: SerialStatusFrame,
    deviceId: string | null,
  ) {
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

  function appendSerialLogToSession(
    frame: SerialLogFrame | SerialLogEntry,
    deviceId: string | null,
  ) {
    if (!deviceId) return;
    const entry = "timestamp" in frame ? frame : serialLogFromFrame(frame);
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId && record.serial?.connected
          ? appendSerialLog(record, entry)
          : record,
      ),
    );
  }

  function appendSerialTraceToSession(
    entry: SerialTraceEvent | SerialTraceEntry,
    deviceId: string | null,
  ) {
    if (!deviceId) return;
    const trace = "timestamp" in entry ? entry : serialTraceFromEvent(entry);
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId && record.serial?.connected
          ? appendSerialTrace(record, trace)
          : record,
      ),
    );
  }

  function appendSerialStatusToSession(
    status: SerialStatusFrame["status"],
    deviceId: string | null,
  ) {
    if (!deviceId) return;
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId && record.serial?.connected
          ? {
              ...record,
              status,
              network: record.network
                ? {
                    ...record.network,
                    state: status.network.state,
                    ipv4: status.network.ipv4,
                    last_error: status.network.last_error,
                  }
                : record.network,
              connectionState: "online",
              streamState: "streaming",
              error: null,
              lastUpdated: new Date().toISOString(),
            }
          : record,
      ),
    );
  }

  async function updateDevdSerialSnapshot(deviceId: string, baseUrl: string) {
    const record = records.find(
      (candidate) => candidate.target.deviceId === deviceId,
    );
    const leaseId = record?.serial?.leaseId;
    if (!leaseId) throw new Error("devd Web lease is missing");
    const session = await getDevdSerialSession(baseUrl, {
      ...DEVD_SERIAL_SESSION_LIMITS,
      leaseId,
    });
    setRecords((current) =>
      current.map((record) =>
        record.target.deviceId === deviceId
          ? mergeDevdSerial(record, baseUrl, session, {
              lease_id: leaseId,
              expires_at: record.serial?.leaseExpiresAt ?? "",
              heartbeat_interval_ms: record.serial?.heartbeatIntervalMs ?? 2000,
              lease_ttl_ms: record.serial?.leaseTtlMs ?? 8000,
            })
          : record,
      ),
    );
  }

  function startDevdLeaseHeartbeat(record: DeviceRecord) {
    const leaseId = record.serial?.leaseId;
    const baseUrl = record.serial?.baseUrl ?? "";
    if (!leaseId) return;
    const existing = devdLeaseHeartbeats.current.get(record.target.deviceId);
    if (existing !== undefined) window.clearInterval(existing);
    const intervalMs = Math.max(
      1000,
      Math.min(record.serial?.heartbeatIntervalMs ?? 2000, 5000),
    );
    const heartbeat = window.setInterval(() => {
      void heartbeatDevdWebLease(baseUrl, leaseId)
        .then((lease) => {
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId &&
              candidate.serial?.leaseId === leaseId
                ? {
                    ...candidate,
                    streamState:
                      candidate.streamState === "error"
                        ? "polling"
                        : candidate.streamState,
                    serial: {
                      ...candidate.serial,
                      connected: true,
                      leaseExpiresAt: lease.expires_at,
                      heartbeatIntervalMs: lease.heartbeat_interval_ms,
                      leaseTtlMs: lease.lease_ttl_ms,
                    },
                  }
                : candidate,
            ),
          );
        })
        .catch((error) => {
          const envelope = toErrorEnvelope(error);
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId &&
              candidate.serial?.leaseId === leaseId
                ? {
                    ...candidate,
                    streamState: "error",
                    error: envelope,
                    lastUpdated: new Date().toISOString(),
                  }
                : candidate,
            ),
          );
        });
    }, intervalMs);
    devdLeaseHeartbeats.current.set(record.target.deviceId, heartbeat);
  }

  const removeDevice = useCallback(
    (deviceId: string) => {
      const record = records.find(
        (candidate) => candidate.target.deviceId === deviceId,
      );
      streams.current.get(deviceId)?.close();
      streams.current.delete(deviceId);
      devdStreams.current.get(deviceId)?.close();
      devdStreams.current.delete(deviceId);
      const heartbeat = devdLeaseHeartbeats.current.get(deviceId);
      if (heartbeat !== undefined) window.clearInterval(heartbeat);
      devdLeaseHeartbeats.current.delete(deviceId);
      void serialSessions.current.get(deviceId)?.close();
      serialSessions.current.delete(deviceId);
      setRecords((current) =>
        current.filter((record) => record.target.deviceId !== deviceId),
      );
      if (record?.serial?.source === "devd") {
        void disconnectDevdSerialDevice(record).catch((error) => {
          console.warn("failed to disconnect devd device", error);
        });
      }
    },
    [records],
  );

  const setDemoSeed = useCallback((seed: DemoSeed) => {
    if (!seedRef.current) return;
    for (const stream of streams.current.values()) stream.close();
    streams.current.clear();
    for (const stream of devdStreams.current.values()) stream.close();
    devdStreams.current.clear();
    for (const heartbeat of devdLeaseHeartbeats.current.values())
      window.clearInterval(heartbeat);
    devdLeaseHeartbeats.current.clear();
    for (const record of records) releaseDevdLeaseForRecord(record, true);
    for (const session of serialSessions.current.values()) void session.close();
    serialSessions.current.clear();
    seedRef.current = seed;
    setActiveDemoSeed(seed);
    setRecords(makeMockRecords(seed));
  }, [records]);

  const resetDemo = useCallback(() => {
    if (!seedRef.current) return;
    for (const stream of streams.current.values()) stream.close();
    streams.current.clear();
    for (const stream of devdStreams.current.values()) stream.close();
    devdStreams.current.clear();
    for (const heartbeat of devdLeaseHeartbeats.current.values())
      window.clearInterval(heartbeat);
    devdLeaseHeartbeats.current.clear();
    for (const record of records) releaseDevdLeaseForRecord(record, true);
    for (const session of serialSessions.current.values()) void session.close();
    serialSessions.current.clear();
    seedRef.current = DEFAULT_DEMO_SEED;
    setActiveDemoSeed(DEFAULT_DEMO_SEED);
    setRecords(makeMockRecords(DEFAULT_DEMO_SEED));
  }, [records]);

  const getSerialAlerts = useCallback(async (deviceId: string): Promise<ActiveAlertsSnapshot> => {
    const session = serialSessions.current.get(deviceId);
    if (!session) throw new Error("serial_session_unavailable: connect the USB device first");
    return session.requestAlerts();
  }, []);

  const muteSerialAlert = useCallback(
    async (deviceId: string, alertId: string, instanceId: number): Promise<void> => {
      const session = serialSessions.current.get(deviceId);
      if (!session) throw new Error("serial_session_unavailable: connect the USB device first");
      await session.muteAlert(alertId, instanceId);
    },
    [],
  );

  const value = useMemo(
    () => ({
      records,
      demoSeed,
      stageDeviceRecord,
      addDevice,
      addDevdDevice,
      confirmDevdCompanionLan,
      dismissDevdCompanionLan,
      connectUsbSerialDevice,
      connectKnownDeviceChannel,
      rememberDiscoveredChannels,
      prepareWebSerialFlashPort,
      attachMockUsbSerialDevice,
      disconnectUsbSerialDevice,
      sendWifiConfig,
      clearWifiConfig,
      setSerialLogLevel,
      setManualChargePrefs,
      refreshChargeControlDetail,
      previewManualCharge,
      controlManualCharge,
      setAdvancedPower,
      resetAdvancedPower,
      getSerialAlerts,
      muteSerialAlert,
      removeDevice,
      refreshDevice,
      setDemoSeed,
      resetDemo,
    }),
    [
      records,
      demoSeed,
      stageDeviceRecord,
      addDevice,
      addDevdDevice,
      confirmDevdCompanionLan,
      dismissDevdCompanionLan,
      connectUsbSerialDevice,
      connectKnownDeviceChannel,
      rememberDiscoveredChannels,
      prepareWebSerialFlashPort,
      attachMockUsbSerialDevice,
      disconnectUsbSerialDevice,
      sendWifiConfig,
      clearWifiConfig,
      setSerialLogLevel,
      setManualChargePrefs,
      refreshChargeControlDetail,
      previewManualCharge,
      controlManualCharge,
      setAdvancedPower,
      resetAdvancedPower,
      getSerialAlerts,
      muteSerialAlert,
      removeDevice,
      refreshDevice,
      setDemoSeed,
      resetDemo,
    ],
  );

  return (
    <DeviceRegistryContext.Provider value={value}>
      {children}
    </DeviceRegistryContext.Provider>
  );
}

function getDemoSeed(initialDemoSeed?: DemoSeed): DemoSeed | null {
  const querySeed = demoQuerySeed();
  if (!querySeed) return null;
  return isDemoSeed(initialDemoSeed) ? initialDemoSeed : querySeed;
}

function loadInitialRecords(seed: DemoSeed | null): DeviceRecord[] {
  const preset = new URLSearchParams(window.location.search).get(
    "stored_target_preset",
  );
  if (isStoredTargetPreset(preset)) {
    return makeStoredTargetPreset(preset).map((target) =>
      recordFromStoredTarget(target),
    );
  }

  if (seed) return makeMockRecords(seed);

  const stored = localStorage.getItem(STORAGE_KEY);
  if (!stored) return [];

  try {
    type StoredDeviceTarget = Omit<DeviceTarget, "transport"> & {
      transport?: DeviceTarget["transport"] | string;
    };
    const targets = JSON.parse(stored) as StoredDeviceTarget[];
    if (!Array.isArray(targets)) return [];
    return targets
      .filter((target) => !target.mock)
      .flatMap((target) => {
        const normalizedTarget = normalizeStoredTarget(target);
        return normalizedTarget
          ? [recordFromStoredTarget(normalizedTarget)]
          : [];
      })
      .reduce(
        (merged, record) => upsertRecord(merged, record),
        [] as DeviceRecord[],
      );
  } catch {
    return [];
  }
}

function persistedTargetsForRecord(record: DeviceRecord): DeviceTarget[] {
  if (
    record.target.mock ||
    record.target.temporary ||
    record.target.transport === "serial"
  )
    return [];
  return [record.target];
}

function recordFromStoredTarget(target: DeviceTarget): DeviceRecord {
  const nextTarget = hydrateRememberedChannels(target);
  return {
    target: nextTarget,
    identity: null,
    network: null,
    settings: null,
    status: null,
    connectionState:
      nextTarget.transport === "serial" ? "offline" : "connecting",
    streamState: nextTarget.transport === "serial" ? "error" : "idle",
    error:
      nextTarget.transport === "serial"
        ? {
            code: "serial_reconnect_required",
            message: "USB CDC devices require a fresh browser permission grant",
            retryable: true,
            details: null,
          }
        : null,
    lastUpdated: null,
    serial:
      nextTarget.transport === "devd"
        ? {
            connected: false,
            source: "devd",
            baseUrl: nextTarget.baseUrl,
            protocol: nextTarget.serialProtocol ?? "mains-aegis.cdc.v1",
            logs: [],
            trace: [],
          }
        : undefined,
  };
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
    settings: result.settings,
    status: result.status,
    connectionState,
    streamState,
    error: null,
    lastUpdated: new Date().toISOString(),
  };
}

function normalizeStoredTarget(
  target: Omit<DeviceTarget, "transport"> & {
    transport?: DeviceTarget["transport"] | string;
  },
): DeviceTarget | null {
  if (target.transport === LEGACY_DEVD_TRANSPORT) {
    return {
      ...target,
      transport: "devd",
      location: target.location || "devd",
    };
  }
  if (
    target.transport === undefined ||
    target.transport === "http" ||
    target.transport === "serial" ||
    target.transport === "devd"
  ) {
    return target as DeviceTarget;
  }
  return null;
}

function recordFromSerialProbe(
  target: DeviceTarget,
  result: Omit<ProbeResult, "settings"> & { settings: DeviceSettings | null },
  protocol: string,
  logs: SerialLogEntry[] = [],
  trace: SerialTraceEntry[] = [],
): DeviceRecord {
  return {
    target,
    identity: result.identity,
    network: result.network,
    settings: result.settings,
    status: result.status,
    connectionState: "online",
    streamState: "streaming",
    error: null,
    lastUpdated: new Date().toISOString(),
    serial: {
      connected: true,
      source: target.mock ? "mock" : "web_serial",
      protocol,
      logs,
      trace,
    },
  };
}

type DevdLeaseSnapshot = Pick<
  DevdWebLease,
  "lease_id" | "expires_at" | "heartbeat_interval_ms" | "lease_ttl_ms"
>;

function recordFromDevdProbe(
  target: DeviceTarget,
  result: ProbeResult,
  session: DevdSerialSession,
  lease: DevdLeaseSnapshot,
): DeviceRecord {
  return mergeDevdSerial(
    recordFromProbe(target, result, "online", "polling"),
    target.baseUrl,
    session,
    lease,
  );
}

function recordFromDevdDeviceSnapshot(
  target: DeviceTarget,
  identity: Identity,
  status: UpsStatus | null,
  settings: DeviceSettings,
  session: Pick<DevdSerialSession, "connected" | "protocol" | "logs" | "trace">,
): DeviceRecord {
  return {
    target,
    identity,
    network: identity.network,
    settings,
    status,
    connectionState: session.connected ? "online" : "offline",
    streamState: "polling",
    error: null,
    lastUpdated: new Date().toISOString(),
    serial: {
      connected: session.connected,
      source: "devd",
      baseUrl: target.baseUrl,
      protocol: session.protocol,
      status,
      logs: session.logs,
      trace: session.trace,
    },
  };
}

function mergeDevdSerial(
  record: DeviceRecord,
  baseUrl: string,
  session: DevdSerialSession,
  lease?: DevdLeaseSnapshot,
): DeviceRecord {
  return {
    ...record,
    target: {
      ...record.target,
      transport: "devd",
      rememberedChannels: mergeRememberedChannels(
        record.target.rememberedChannels,
        {
          devd: {
            baseUrl,
            devdDeviceId: record.target.rememberedChannels?.devd?.devdDeviceId,
            seenAt: new Date().toISOString(),
            transport:
              record.target.rememberedChannels?.devd?.transport ?? "usb",
          },
        },
      ),
      serialProtocol: session.protocol,
    },
    connectionState: session.connected ? "online" : record.connectionState,
    error: session.connected ? null : record.error,
    lastUpdated: new Date().toISOString(),
    status: session.status ?? record.status,
    settings: session.settings,
    network:
      record.network && session.status
        ? {
            ...record.network,
            state: session.status.network.state,
            ipv4: session.status.network.ipv4,
            last_error: session.status.network.last_error,
          }
        : record.network,
    serial: {
      connected: session.connected,
      source: "devd",
      baseUrl,
      leaseId: lease?.lease_id ?? record.serial?.leaseId,
      leaseExpiresAt: lease?.expires_at ?? record.serial?.leaseExpiresAt,
      heartbeatIntervalMs:
        lease?.heartbeat_interval_ms ?? record.serial?.heartbeatIntervalMs,
      leaseTtlMs: lease?.lease_ttl_ms ?? record.serial?.leaseTtlMs,
      protocol: session.protocol,
      status: session.status ?? null,
      logs: session.logs,
      trace: session.trace,
    },
  };
}

async function disconnectDevdSerialDevice(record: DeviceRecord): Promise<void> {
  const baseUrl = record.serial?.baseUrl ?? "";
  if (record.serial?.leaseId) {
    await releaseDevdWebLease(baseUrl, record.serial.leaseId);
    return;
  }
  const rememberedDeviceId =
    record.target.rememberedChannels?.devd?.devdDeviceId;
  if (rememberedDeviceId) {
    await disconnectDevdDevice(rememberedDeviceId, baseUrl);
    return;
  }
  const devices = await listDevdDevices(baseUrl);
  const devdDevice = devices.devices.find(
    (device) => devdLogicalDeviceId(device) === record.target.deviceId,
  );
  if (!devdDevice) return;
  await disconnectDevdDevice(devdDevice.id, baseUrl);
}

function releaseDevdLeaseForRecord(record: DeviceRecord, keepalive = false) {
  if (record.serial?.source !== "devd" || !record.serial.leaseId) return;
  void releaseDevdWebLease(
    record.serial.baseUrl ?? "",
    record.serial.leaseId,
    keepalive,
  ).catch(() => undefined);
}

function upsertRecord(
  records: DeviceRecord[],
  record: DeviceRecord,
): DeviceRecord[] {
  const existing = records.find(
    (candidate) => candidate.target.deviceId === record.target.deviceId,
  );
  const next = records.filter(
    (candidate) => candidate.target.deviceId !== record.target.deviceId,
  );
  next.push(existing ? mergeDeviceRecord(existing, record) : record);
  return next;
}

function mergeDeviceRecord(
  existing: DeviceRecord,
  incoming: DeviceRecord,
): DeviceRecord {
  const preferIncomingTarget =
    incoming.connectionState === "online" ||
    incoming.connectionState === "connecting";
  return {
    ...existing,
    ...incoming,
    target: preferIncomingTarget
      ? {
          ...incoming.target,
          alias: incoming.target.alias || existing.target.alias,
          location: incoming.target.location || existing.target.location,
          preferredTransport:
            incoming.target.preferredTransport ??
            existing.target.preferredTransport,
          rememberedChannels: mergeRememberedChannels(
            existing.target.rememberedChannels,
            incoming.target.rememberedChannels,
          ),
        }
      : {
          ...existing.target,
          preferredTransport:
            incoming.target.preferredTransport ??
            existing.target.preferredTransport,
          rememberedChannels: mergeRememberedChannels(
            existing.target.rememberedChannels,
            incoming.target.rememberedChannels,
          ),
          serialProtocol:
            incoming.target.serialProtocol ?? existing.target.serialProtocol,
        },
    serial: incoming.serial ?? existing.serial,
    settings: incoming.settings ?? existing.settings,
    status: incoming.status ?? existing.status,
    chargeControlDetail:
      incoming.chargeControlDetail ?? existing.chargeControlDetail,
    network: incoming.network ?? existing.network,
    identity: incoming.identity ?? existing.identity,
    connectionState:
      incoming.connectionState === "online" ||
      existing.connectionState === "online"
        ? "online"
        : incoming.connectionState,
    streamState:
      incoming.streamState === "streaming" ||
      existing.streamState === "streaming"
        ? "streaming"
        : incoming.streamState,
    error: incoming.error ?? existing.error,
    lastUpdated: incoming.lastUpdated ?? existing.lastUpdated,
  };
}

function isDevdSerial(
  record: DeviceRecord,
): record is DeviceRecord & {
  serial: NonNullable<DeviceRecord["serial"]> & {
    source: "devd";
    baseUrl: string;
  };
} {
  return (
    record.serial?.source === "devd" &&
    record.serial.baseUrl !== undefined &&
    record.serial.baseUrl !== null
  );
}

function isManageableDevdDevice(device: DevdDevice): boolean {
  if (device.transport === "native_serial") return Boolean(device.port_path);
  if (device.transport !== "lan") return false;
  return (
    isMainsAegisLanDevice(device) &&
    (device.lan_conflict_addresses?.length ?? 0) === 0
  );
}

function devdLogicalDeviceId(device: DevdDevice): string | null {
  return (
    device.binding?.logical_device_id ?? device.identity?.device_id ?? null
  );
}

function isMainsAegisLanDevice(device: DevdDevice): boolean {
  return (
    device.transport === "lan" &&
    device.identity?.firmware.protocol === "mains-aegis.cdc.v1"
  );
}

function devdLanBaseUrl(
  device: DevdDevice,
  identity: Identity | null,
): string | null {
  const candidate =
    device.lan_address?.trim() ||
    identity?.network.ipv4?.trim() ||
    identity?.hostname_fqdn?.trim() ||
    identity?.hostname?.trim() ||
    "";
  return candidate ? normalizeBaseUrl(candidate) : null;
}

function devdBaseUrlForRecord(record: DeviceRecord): string | null {
  if (record.serial?.source === "devd") return record.serial.baseUrl ?? "";
  if (record.target.transport === "devd") return record.target.baseUrl ?? "";
  if (record.target.rememberedChannels?.devd)
    return record.target.rememberedChannels.devd.baseUrl;
  return null;
}

function devdDeviceIdForRecord(record: DeviceRecord): string | null {
  return record.target.rememberedChannels?.devd?.devdDeviceId ?? null;
}

function devdLeaseIdForRecord(record: DeviceRecord): string | null {
  return record.serial?.source === "devd"
    ? (record.serial.leaseId ?? null)
    : null;
}

function isDirectLanRecord(record: DeviceRecord): boolean {
  return (record.target.transport ?? "http") === "http";
}

function rememberedHttpBaseUrl(record: DeviceRecord): string | null {
  return rememberedHttpBaseUrls(record)[0] ?? null;
}

function rememberedHttpBaseUrls(record: DeviceRecord): string[] {
  const candidates = [
    record.target.rememberedChannels?.http?.baseUrl,
    record.target.rememberedChannels?.http?.fallbackBaseUrl,
    (record.target.transport ?? "http") === "http"
      ? record.target.baseUrl
      : null,
  ];
  const normalized: string[] = [];
  for (const candidate of candidates) {
    if (!candidate) continue;
    const baseUrl = normalizeBaseUrl(candidate);
    if (!baseUrl || normalized.includes(baseUrl)) continue;
    normalized.push(baseUrl);
  }
  return normalized;
}

async function withRememberedHttpFallback<T>(
  record: DeviceRecord,
  operation: (baseUrl: string) => Promise<T>,
): Promise<T> {
  const baseUrls = rememberedHttpBaseUrls(record);
  let lastError: unknown = null;
  for (const baseUrl of baseUrls) {
    try {
      return await operation(baseUrl);
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError ?? new Error("No remembered HTTP channel is available");
}

function rememberedDevdChannel(
  record: DeviceRecord,
): NonNullable<DeviceTarget["rememberedChannels"]>["devd"] | null {
  if (record.target.rememberedChannels?.devd)
    return record.target.rememberedChannels.devd;
  if (record.target.transport === "devd") {
    return {
      baseUrl: record.target.baseUrl,
      seenAt: record.target.addedAt,
    };
  }
  if (
    record.serial?.source === "devd" &&
    record.serial.baseUrl !== undefined &&
    record.serial.baseUrl !== null
  ) {
    return {
      baseUrl: record.serial.baseUrl,
      seenAt: record.lastUpdated ?? record.target.addedAt,
    };
  }
  return null;
}

function isTransportAvailable(
  record: DeviceRecord,
  transport: DeviceChannelTransport,
  sessions: Map<string, WebSerialTransport>,
): boolean {
  if (transport === "http") return Boolean(rememberedHttpBaseUrl(record));
  if (transport === "devd") return devdBaseUrlForRecord(record) !== null;
  return (
    record.serial?.connected === true || sessions.has(record.target.deviceId)
  );
}

function resolvePreferredTransport(
  record: DeviceRecord,
  sessions: Map<string, WebSerialTransport>,
): DeviceChannelTransport {
  const preferred = record.target.preferredTransport;
  if (preferred && isTransportAvailable(record, preferred, sessions))
    return preferred;
  if (isTransportAvailable(record, "devd", sessions)) return "devd";
  if (isTransportAvailable(record, "http", sessions)) return "http";
  return "serial";
}

function mergeRememberedChannels(
  existing: DeviceTarget["rememberedChannels"],
  incoming:
    | DeviceTarget["rememberedChannels"]
    | Partial<NonNullable<DeviceTarget["rememberedChannels"]>>
    | undefined,
): DeviceTarget["rememberedChannels"] {
  if (!existing && !incoming) return undefined;
  return {
    http: mergeRememberedHttpChannel(existing?.http, incoming?.http),
    devd: incoming?.devd ?? existing?.devd,
    serial: incoming?.serial ?? existing?.serial,
  };
}

function mergeRememberedHttpChannel(
  existing:
    | NonNullable<NonNullable<DeviceTarget["rememberedChannels"]>["http"]>
    | undefined,
  incoming:
    | NonNullable<NonNullable<DeviceTarget["rememberedChannels"]>["http"]>
    | undefined,
) {
  if (!existing) return incoming;
  if (!incoming) return existing;
  const preserveConfirmedBaseUrl =
    Boolean(existing.mdnsHost) && !incoming.mdnsHost;
  return {
    baseUrl: preserveConfirmedBaseUrl
      ? existing.baseUrl
      : (incoming.baseUrl ?? existing.baseUrl),
    fallbackBaseUrl: incoming.fallbackBaseUrl ?? existing.fallbackBaseUrl,
    seenAt: incoming.seenAt ?? existing.seenAt,
    source: incoming.source ?? existing.source,
    mdnsHost: incoming.mdnsHost ?? existing.mdnsHost,
  };
}

function hydrateRememberedChannels(target: DeviceTarget): DeviceTarget {
  const rememberedChannels = mergeRememberedChannels(
    target.rememberedChannels,
    target.transport === "devd"
      ? {
          devd: {
            baseUrl: target.baseUrl,
            seenAt: target.addedAt,
          },
        }
      : target.transport === "http" || target.transport === undefined
        ? {
            http: {
              baseUrl: target.baseUrl,
              seenAt: target.addedAt,
              mdnsHost: target.rememberedChannels?.http?.mdnsHost,
            },
          }
        : {
            serial: {
              seenAt: target.addedAt,
            },
          },
  );
  return {
    ...target,
    rememberedChannels,
  };
}

function unavailableChannelError(
  transport: DeviceChannelTransport,
): AddDeviceResult {
  return {
    ok: false,
    error: {
      code: `${transport}_channel_unavailable`,
      message: unavailableChannelMessage(transport),
      retryable: true,
      details: { transport },
    },
  };
}

function unavailableCommandChannel(
  transport: DeviceChannelTransport,
): CommandResult {
  return {
    ok: false,
    error: {
      code: `${transport}_channel_unavailable`,
      message: unavailableChannelMessage(transport),
      retryable: true,
      details: { transport },
    },
  };
}

function unavailableChannelMessage(transport: DeviceChannelTransport): string {
  if (transport === "devd")
    return "No remembered mains-aegis-devd USB channel is available for this device";
  if (transport === "http")
    return "No remembered LAN HTTP channel is available for this device";
  return "No remembered Web Serial channel is available for this device";
}

type DeviceSettingsPatch = {
  wifi_configured?: boolean | null;
  wifi_ssid?: string | null;
  log_level?: DeviceSettings["log_level"];
  manual_charge?: DeviceSettings["manual_charge"];
  charge_capabilities?: DeviceSettings["charge_capabilities"];
  advanced_power?: DeviceSettings["advanced_power"];
  advanced_power_capabilities?: DeviceSettings["advanced_power_capabilities"];
};

type DeviceStatusPatch = {
  charge_control?: UpsStatus["charge_control"];
  chargeControlDetail?: ChargeControlDetail | null;
};

function defaultDeviceSettings(): DeviceSettings {
  const ratedVoutMv = 12_000;
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
    charge_capabilities: {
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
      auto_path_priority: ["usbc_pd_high_power", "dcin", "usbc"],
    },
    advanced_power: buildAdvancedPowerDefaults(ratedVoutMv),
    advanced_power_capabilities: buildAdvancedPowerCapabilities(ratedVoutMv),
  };
}

function manualChargePreviewInput(
  prefs: DeviceSettings["manual_charge"],
): {
  target: DeviceSettings["manual_charge"]["target"];
  current_ma: number;
  timer_minutes: number;
  power_path: DeviceSettings["manual_charge"]["power_path"];
} {
  return {
    target: prefs.target,
    current_ma:
      prefs.speed === "ma_100"
        ? 100
        : prefs.speed === "ma_1000"
          ? 1_000
          : 500,
    timer_minutes: prefs.timer_h * 60,
    power_path: prefs.power_path ?? "auto",
  };
}

export async function loadUsbProbeSettings(
  hello: Pick<SerialHelloFrame, "capabilities">,
  transport: Pick<WebSerialTransport, "requestSettings">,
): Promise<DeviceSettings | null> {
  if (hello.capabilities?.settings !== true) {
    return null;
  }
  try {
    return await transport.requestSettings();
  } catch (error) {
    if (errorFromSerialFailure(error).code === "unsupported_operation") {
      return null;
    }
    throw error;
  }
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

function firmwareMismatchError(identity: Identity): DeviceRecord["error"] {
  return {
    code: "firmware_artifact_mismatch",
    message: `Connected firmware build ${identity.firmware.build_id} does not match any available firmware artifact. defmt decode and safe diagnostics may be wrong. Ignore only if you intentionally run an out-of-catalog build.`,
    retryable: false,
    details: {
      device_id: identity.device_id,
      build_id: identity.firmware.build_id,
      git_sha: identity.firmware.git_sha,
      build_profile: identity.firmware.build_profile,
      features: identity.firmware.features,
    },
  };
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join(" ");
}

function appendSerialLog(
  record: DeviceRecord,
  entry: SerialLogEntry,
): DeviceRecord {
  if (!record.serial) return record;
  return {
    ...record,
    serial: {
      ...record.serial,
      logs: [...record.serial.logs, entry].slice(-200),
    },
  };
}

function appendSerialTrace(
  record: DeviceRecord,
  entry: SerialTraceEntry,
): DeviceRecord {
  if (!record.serial) return record;
  return {
    ...record,
    serial: {
      ...record.serial,
      trace: [...record.serial.trace, entry].slice(
        -DEVD_SERIAL_SESSION_LIMITS.traceLimit,
      ),
    },
  };
}

function updateSerialSettings(
  record: DeviceRecord,
  patch: DeviceSettingsPatch,
  target: string,
  message: string,
): DeviceRecord {
  const nextSettings = mergeDeviceSettings(
    record.settings ?? defaultDeviceSettings(),
    patch,
  );
  const nextRecord: DeviceRecord = {
    ...record,
    settings: nextSettings,
    lastUpdated: new Date().toISOString(),
  };
  if (!record.serial) return nextRecord;
  return appendSerialLog(
    nextRecord,
    serialLogFromFrame({ type: "log", level: "info", target, message }),
  );
}

function patchSerialStatusRecord(
  record: DeviceRecord,
  patch: DeviceStatusPatch,
  target: string,
  message: string,
): DeviceRecord {
  const nextStatus = mergeDeviceStatus(record.status, patch);
  const nextRecord: DeviceRecord = {
    ...record,
    status: nextStatus,
    chargeControlDetail:
      patch.chargeControlDetail ?? record.chargeControlDetail ?? null,
    error: null,
    lastUpdated: new Date().toISOString(),
  };
  if (!record.serial) return nextRecord;
  return appendSerialLog(
    nextRecord,
    serialLogFromFrame({ type: "log", level: "info", target, message }),
  );
}

function mergeLanDeviceSnapshot(
  record: DeviceRecord,
  status: UpsStatus | undefined,
  settings: DeviceSettings,
  message: string,
): DeviceRecord {
  const nextRecord: DeviceRecord = {
    ...record,
    status: status ?? record.status,
    settings,
    network:
      status && record.network
        ? {
            ...record.network,
            state: status.network.state,
            ipv4: status.network.ipv4,
            last_error: status.network.last_error,
          }
        : record.network,
    connectionState: "online",
    error: null,
    lastUpdated: new Date().toISOString(),
  };
  if (!record.serial) return nextRecord;
  return appendSerialLog(
    nextRecord,
    serialLogFromFrame({
      type: "log",
      level: "info",
      target: "lan_http",
      message,
    }),
  );
}

function mergeDeviceSettings(
  current: DeviceSettings,
  patch: DeviceSettingsPatch,
): DeviceSettings {
  return {
    wifi: {
      configured: patch.wifi_configured ?? current.wifi.configured,
      ssid: patch.wifi_ssid ?? current.wifi.ssid,
    },
    log_level: patch.log_level ?? current.log_level,
    manual_charge: patch.manual_charge ?? current.manual_charge,
    charge_capabilities:
      patch.charge_capabilities ?? current.charge_capabilities,
    advanced_power: patch.advanced_power ?? current.advanced_power,
    advanced_power_capabilities:
      patch.advanced_power_capabilities ?? current.advanced_power_capabilities,
  };
}

function mergeDeviceStatus(
  current: UpsStatus | null,
  patch: DeviceStatusPatch,
): UpsStatus | null {
  if (!current) return current;
  return {
    ...current,
    charge_control: patch.charge_control ?? current.charge_control,
  };
}

function chargeControlSummaryFromDetail(
  detail: ChargeControlDetail,
): NonNullable<UpsStatus["charge_control"]> {
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

function chargeControlPatchFromDetail(
  detail: ChargeControlDetail,
): DeviceStatusPatch {
  return {
    charge_control: chargeControlSummaryFromDetail(detail),
    chargeControlDetail: detail,
  };
}

function chargeControlDetailFromErrorDetails(
  details: unknown,
): ChargeControlDetail | null {
  if (!details || typeof details !== "object") return null;
  if (
    "summary" in details &&
    "readiness" in details &&
    "telemetry" in details &&
    "evidence" in details
  ) {
    return details as ChargeControlDetail;
  }
  return null;
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

async function waitForSerialWifiConnected(
  session: { requestStatus: () => Promise<UpsStatus> },
  ssid: string,
  onProgress?: (progress: WifiProvisioningProgress) => void,
): Promise<UpsStatus> {
  return waitForSerialWifiState(session, "connected", ssid, onProgress);
}

async function waitForSerialWifiDisabled(
  session: { requestStatus: () => Promise<UpsStatus> },
  onProgress?: (progress: WifiProvisioningProgress) => void,
): Promise<UpsStatus> {
  return waitForSerialWifiState(session, "disabled", undefined, onProgress);
}

async function waitForHttpWifiConnected(
  baseUrl: string,
  ssid: string,
  onProgress?: (progress: WifiProvisioningProgress) => void,
): Promise<UpsStatus> {
  return waitForSerialWifiState(
    { requestStatus: () => getStatus(baseUrl) },
    "connected",
    ssid,
    onProgress,
  );
}

async function waitForHttpWifiDisabled(
  baseUrl: string,
  onProgress?: (progress: WifiProvisioningProgress) => void,
): Promise<UpsStatus> {
  return waitForSerialWifiState(
    { requestStatus: () => getStatus(baseUrl) },
    "disabled",
    undefined,
    onProgress,
  );
}

async function waitForSerialWifiState(
  session: { requestStatus: () => Promise<UpsStatus> },
  expectedState: UpsStatus["network"]["state"],
  ssid?: string,
  onProgress?: (progress: WifiProvisioningProgress) => void,
): Promise<UpsStatus> {
  const deadline = Date.now() + 45_000;
  let lastNetwork: UpsStatus["network"] | null = null;
  while (Date.now() < deadline) {
    const status = await session.requestStatus();
    lastNetwork = status.network;
    onProgress?.(wifiProgressFromNetwork(status.network, expectedState, ssid));
    if (status.network.state === expectedState) return status;
    if (expectedState !== "disabled" && status.network.state === "error") {
      throw new Error(
        `wifi_connect_failed: ${status.network.last_error ?? "unknown"}`,
      );
    }
    await new Promise((resolve) => window.setTimeout(resolve, 750));
  }
  throw new Error(
    `wifi_${expectedState}_timeout: last network state ${JSON.stringify(lastNetwork)}`,
  );
}

function wifiProgressFromNetwork(
  network: UpsStatus["network"],
  expectedState: UpsStatus["network"]["state"],
  ssid?: string,
): WifiProvisioningProgress {
  if (expectedState === "disabled") {
    return network.state === "disabled"
      ? {
          phase: "disabled",
          message: "WiFi credentials cleared and WiFi disconnected",
          network,
        }
      : {
          phase: "clearing",
          message: "Disconnecting WiFi and clearing runtime credentials",
          network,
        };
  }
  if (network.state === "connected") {
    return network.ipv4
      ? {
          phase: "connected",
          message: wifiConnectedMessage(ssid ?? "network", network),
          network,
        }
      : {
          phase: "ip",
          message: "WiFi link is up. Waiting for an IP address",
          network,
        };
  }
  if (network.state === "connecting") {
    return {
      phase: "connecting",
      message: ssid
        ? `Connecting to ${ssid} and waiting for an IP address`
        : "Connecting to WiFi and waiting for an IP address",
      network,
    };
  }
  return {
    phase: "starting",
    message: "Starting WiFi with the saved credentials",
    network,
  };
}

function wifiConnectedMessage(
  ssid: string,
  network: { state: string; ipv4: string | null },
): string {
  return network.ipv4
    ? `WiFi connected to ${ssid} at ${network.ipv4}`
    : `WiFi connected to ${ssid}`;
}

function wifiDisabledMessage(network: { state: string }): string {
  return network.state === "disabled"
    ? "WiFi credentials cleared and WiFi disconnected"
    : "WiFi credentials cleared";
}
