import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  bridgeAuthRequired,
  bridgeAuthToken,
  clearDevdWifiConfig,
  connectDevdDevice,
  createDevdWebLease,
  decodeDefmtFrame,
  disconnectDevdDevice,
  getDevdSerialSession,
  heartbeatDevdWebLease,
  getStatus,
  listDevdDevices,
  normalizeBaseUrl,
  probeDevice,
  releaseDevdWebLease,
  scanDevdDevices,
  sendDevdWifiConfig,
  setDevdLogLevel,
  setDevdManualChargePrefs,
  saveBridgeAuthToken,
  subscribeDevdSerialEvents,
  toErrorEnvelope,
  type DevdSerialEventStream,
  type DevdSerialSession,
} from "../api/client";
import { subscribeStatusStream, type StatusStream } from "../api/statusStream";
import type { DevdWebLease, DeviceRecord, DeviceTarget, Identity, ProbeResult, SafeSettingsState, SerialLogEntry, SerialTraceEntry, UpsStatus } from "../api/types";
import { isDemoSeed, makeMockRecord, makeMockRecords, makeMockUsbSerialRecord, type DemoSeed } from "../fixtures/mockDevices";
import {
  findBundledFirmwareArtifact,
  findFirmwareArtifactForIdentity,
  firmwareArtifactElfPath,
  firmwareCatalogSourceLabel,
} from "../firmware/catalog";
import {
  errorFromSerialFailure,
  isWebSerialSupported,
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
  type CommandResult,
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

export function DeviceRegistryProvider({ children }: { children: React.ReactNode }) {
  const seedRef = useRef<DemoSeed | null>(getDemoSeed());
  const [demoSeed, setDemoSeed] = useState<DemoSeed | null>(seedRef.current);
  const [records, setRecords] = useState<DeviceRecord[]>(() => loadInitialRecords(seedRef.current));
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
      const nextSeed = getDemoSeed();
      if (seedRef.current === nextSeed) return;
      seedRef.current = nextSeed;
      setDemoSeed(nextSeed);
      for (const stream of streams.current.values()) stream.close();
      streams.current.clear();
      for (const stream of devdStreams.current.values()) stream.close();
      devdStreams.current.clear();
      for (const heartbeat of devdLeaseHeartbeats.current.values()) window.clearInterval(heartbeat);
      devdLeaseHeartbeats.current.clear();
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
        let handledByDevd = false;
        setRecords((current) =>
          current.map((record) => {
            if (record.target.deviceId !== deviceId || !isDevdSerial(record)) return record;
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
        if (!handledByDevd) setRecordError(deviceId, error);
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
              : mergeDeviceRecord(record, makeMockRecord(record.target))
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
      if (target.transport === "devd") {
        setRecordError(deviceId, {
          code: "web_session_required",
          message: "Reconnect devd to create a fresh Web USB lease",
          retryable: false,
          details: null,
        });
        return;
      }
      const result = await probeDevice(target.baseUrl, undefined, target.bridgeAuth ? { bridgeAuth: true } : undefined);
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
  }, [records, setRecordError]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      for (const record of records) {
        if (record.target.transport !== "devd" && !streams.current.has(record.target.deviceId)) {
          void refreshDevice(record.target.deviceId);
        }
      }
    }, 10000);
    return () => window.clearInterval(interval);
  }, [records, refreshDevice]);

  useEffect(() => {
    if (demoSeed) return;
    for (const record of records) {
      const devdBaseUrl = record.serial?.source === "devd" ? record.serial.baseUrl : null;
      const leaseId = record.serial?.leaseId;
      if (!devdBaseUrl || !leaseId || !record.serial?.connected || devdStreams.current.has(record.target.deviceId)) continue;
      const subscription = subscribeDevdSerialEvents(devdBaseUrl, leaseId, {
        onEvent: (event) => {
          if (event.kind === "serial_trace" && event.payload.trace) appendSerialTraceToSession(event.payload.trace, record.target.deviceId);
          if (event.kind === "serial_log" && event.payload.log) appendSerialLogToSession(event.payload.log, record.target.deviceId);
          if (event.kind === "serial_status" && event.payload.status) appendSerialStatusToSession(event.payload.status, record.target.deviceId);
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId
                ? { ...candidate, streamState: "streaming", connectionState: "online", error: null, lastUpdated: new Date().toISOString() }
                : candidate,
            ),
          );
        },
        onError: () => {
          devdStreams.current.get(record.target.deviceId)?.close();
          devdStreams.current.delete(record.target.deviceId);
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId ? { ...candidate, streamState: "polling" } : candidate,
            ),
          );
          void updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
        },
      });
      devdStreams.current.set(record.target.deviceId, subscription);
    }
    for (const [deviceId, stream] of devdStreams.current.entries()) {
      if (!records.some((record) => record.target.deviceId === deviceId && record.serial?.source === "devd" && record.serial.connected)) {
        stream.close();
        devdStreams.current.delete(deviceId);
      }
    }
  }, [demoSeed, records]);

  useEffect(() => {
    for (const record of records) {
      if (
        record.target.transport === "serial" ||
        record.target.transport === "devd" ||
        record.target.mock ||
        !record.identity?.capabilities.sse ||
        record.streamState !== "idle" ||
        streams.current.has(record.target.deviceId)
      ) {
        continue;
      }

      const bridgeAuth = record.target.bridgeAuth ? { bridgeAuth: true } : undefined;
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
          void getStatus(record.target.baseUrl, undefined, bridgeAuth)
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
      }, bridgeAuth);

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
      for (const heartbeat of devdLeaseHeartbeats.current.values()) window.clearInterval(heartbeat);
      devdLeaseHeartbeats.current.clear();
      for (const record of records) releaseDevdLeaseForRecord(record, true);
      for (const session of serialSessions.current.values()) void session.close();
      serialSessions.current.clear();
    };
  }, []);

  const addDevice = useCallback(async (input: AddDeviceInput): Promise<AddDeviceResult> => {
    const baseUrl = normalizeBaseUrl(input.target);

    try {
      if (input.bridgeAuthToken !== undefined) saveBridgeAuthToken(baseUrl, input.bridgeAuthToken);
      const bridgeAuth = await bridgeAuthRequired(baseUrl);
      if (bridgeAuth && !bridgeAuthToken(baseUrl)) {
        return {
          ok: false,
          error: {
            code: "bridge_auth_token_required",
            message: "This bridge requires an auth token before probing",
            retryable: false,
            details: null,
          },
        };
      }
      const result = await probeDevice(baseUrl, undefined, bridgeAuth ? { bridgeAuth: true } : undefined);
      const target: DeviceTarget = {
        deviceId: result.identity.device_id,
        baseUrl,
        alias: input.alias?.trim() || result.identity.hostname,
        location: input.location?.trim() || "Unassigned",
        addedAt: new Date().toISOString(),
        bridgeAuth: bridgeAuth || undefined,
      };
      const record = recordFromProbe(target, result, "online", result.identity.capabilities.sse ? "idle" : "polling");
      setRecords((current) => upsertRecord(current, record));
      return { ok: true, record };
    } catch (error) {
      return { ok: false, error: toErrorEnvelope(error) };
    }
  }, []);

  const addDevdDevice = useCallback(async (input: AddDeviceInput): Promise<AddDeviceResult> => {
    const baseUrl = normalizeBaseUrl(input.target);
    let pendingLeaseId: string | null = null;

    try {
      if (input.bridgeAuthToken !== undefined) saveBridgeAuthToken(baseUrl, input.bridgeAuthToken);
      const bridgeAuth = await bridgeAuthRequired(baseUrl);
      if (bridgeAuth && !bridgeAuthToken(baseUrl)) {
        return {
          ok: false,
          error: {
            code: "bridge_auth_token_required",
            message: "This bridge requires an auth token before probing",
            retryable: false,
            details: null,
          },
        };
      }
      const scan = await scanDevdDevices(baseUrl);
      const nativeDevices = scan.devices.filter((device) => device.transport === "native_serial" && device.port_path);
      const selectedDevice = input.devdDeviceId
        ? nativeDevices.find((device) => device.id === input.devdDeviceId)
        : nativeDevices.length === 1
          ? nativeDevices[0]
          : null;
      if (!selectedDevice) {
        return {
          ok: false,
          error: {
            code: nativeDevices.length === 0 ? "devd_no_usb_device" : "devd_multiple_usb_devices",
            message:
              nativeDevices.length === 0
                ? "No USB CDC device is available through mains-aegis-devd"
                : "Multiple USB CDC devices are available; select a device before adding the devd control surface",
            retryable: false,
            details: { devices: nativeDevices },
          },
        };
      }
      const lease = await createDevdWebLease(baseUrl, selectedDevice.id);
      pendingLeaseId = lease.lease_id;
      const result = await probeDevice(baseUrl, lease.lease_id, bridgeAuth ? { bridgeAuth: true } : undefined);
      const firmwareMatch = await findFirmwareArtifactForIdentity(result.identity);
      if (!firmwareMatch && !input.ignoreFirmwareMismatch) {
        await releaseDevdWebLease(baseUrl, lease.lease_id).catch(() => undefined);
        pendingLeaseId = null;
        return {
          ok: false,
          error: firmwareMismatchError(result.identity),
        };
      }
      const session = await getDevdSerialSession(baseUrl, { ...DEVD_SERIAL_SESSION_LIMITS, leaseId: lease.lease_id });
      const target: DeviceTarget = {
        deviceId: result.identity.device_id,
        baseUrl,
        alias: input.alias?.trim() || result.identity.hostname,
        location: input.location?.trim() || "devd",
        addedAt: new Date().toISOString(),
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
      if (pendingLeaseId) await releaseDevdWebLease(baseUrl, pendingLeaseId).catch(() => undefined);
      return { ok: false, error: toErrorEnvelope(error) };
    }
  }, []);

  const connectUsbSerialDevice = useCallback(
    async (input: Pick<AddDeviceInput, "alias" | "location" | "ignoreFirmwareMismatch"> = {}): Promise<AddDeviceResult> => {
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
          onDefmtLog: (decoded) => {
            const log = serialLogFromFrame({
              type: "log",
              level: decoded.level,
              target: decoded.target,
              message: decoded.message,
            });
            const deviceId = transportRef ? findSessionDeviceId(transportRef) : null;
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
          serialProtocol: hello.protocol,
        };
        serialSessions.current.set(identity.device_id, transport);
        openedTransport = null;
        const decoderArtifact =
          firmwareMatch?.source === "github_release" ? await findBundledFirmwareArtifact(identity) : firmwareMatch?.artifact;
        const bundledElfPath = decoderArtifact ? firmwareArtifactElfPath(decoderArtifact) : null;
        transport.setDefmtDecoder(
          bundledElfPath
            ? (frame) => decodeDefmtFrame({ elf_path: bundledElfPath, frame_hex: bytesToHex(frame) })
            : null,
        );
        const record = recordFromSerialProbe(
          target,
          { identity, network: identity.network, status },
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

  const prepareWebSerialFlashPort = useCallback(async (deviceId: string): Promise<SerialPortLike | null> => {
    const session = serialSessions.current.get(deviceId);
    if (!session) return null;
    serialSessions.current.delete(deviceId);
    return session.releasePort();
  }, []);

  const sendWifiConfig = useCallback(async (deviceId: string, input: WifiConfigInput, onProgress?: (progress: WifiProvisioningProgress) => void): Promise<CommandResult> => {
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    if (!record) return serialCommandUnavailable();
    if (record.target.mock) {
      onProgress?.({ phase: "connected", message: `WiFi connected to ${input.ssid} at 192.168.31.42`, network: { state: "connected", ipv4: "192.168.31.42", last_error: null } });
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
    const devdBaseUrl = devdBaseUrlForRecord(record);
    if (devdBaseUrl !== null) {
      try {
        const leaseId = devdLeaseIdForRecord(record);
        if (!leaseId) return serialCommandUnavailable();
        onProgress?.({ phase: "saving", message: "Writing WiFi credentials to hardware" });
        onProgress?.({ phase: "connecting", message: `Connecting to ${input.ssid} and waiting for an IP address` });
        const applyResult = await sendDevdWifiConfig(devdBaseUrl, record.target.deviceId, leaseId, input);
        await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
        const message = wifiConnectedMessage(input.ssid, applyResult.network);
        onProgress?.({ phase: applyResult.network.ipv4 ? "connected" : "ip", message, network: applyResult.network });
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? updateSerialSettings(candidate, {
                  wifi_configured: true,
                  wifi_ssid: input.ssid,
                }, "wifi_config", message)
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
    if (!record.serial) return serialCommandUnavailable();
    const session = serialSessions.current.get(deviceId);
    if (!session) return serialCommandUnavailable();
    try {
      onProgress?.({ phase: "saving", message: "Writing WiFi credentials to hardware" });
      await session.setWifiConfig(input.ssid, input.psk);
      onProgress?.({ phase: "connecting", message: `Connecting to ${input.ssid} and waiting for an IP address` });
      const status = await waitForSerialWifiConnected(session, input.ssid, onProgress);
      const message = wifiConnectedMessage(input.ssid, status.network);
      onProgress?.({ phase: status.network.ipv4 ? "connected" : "ip", message, network: status.network });
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(candidate, {
                wifi_configured: true,
                wifi_ssid: input.ssid,
              }, "wifi_config", message)
            : candidate,
        ),
      );
      return { ok: true, message, network: status.network };
    } catch (error) {
      const envelope = errorFromSerialFailure(error);
      setSerialCommandError(deviceId, envelope);
      return { ok: false, error: envelope };
    }
  }, [records, setSerialCommandError]);

  const clearWifiConfig = useCallback(async (deviceId: string, onProgress?: (progress: WifiProvisioningProgress) => void): Promise<CommandResult> => {
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    if (!record) return serialCommandUnavailable();
    if (record.target.mock) {
      onProgress?.({ phase: "disabled", message: "WiFi credentials cleared and WiFi disconnected", network: { state: "disabled", ipv4: null, last_error: null } });
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(candidate, { wifi_configured: false, wifi_ssid: null }, "wifi_config", "WiFi credentials cleared")
            : candidate,
        ),
      );
      return { ok: true };
    }
    const devdBaseUrl = devdBaseUrlForRecord(record);
    if (devdBaseUrl !== null) {
      try {
        const leaseId = devdLeaseIdForRecord(record);
        if (!leaseId) return serialCommandUnavailable();
        onProgress?.({ phase: "clearing", message: "Clearing WiFi credentials from hardware" });
        const applyResult = await clearDevdWifiConfig(devdBaseUrl, record.target.deviceId, leaseId);
        await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
        const message = wifiDisabledMessage(applyResult.network);
        onProgress?.({ phase: "disabled", message, network: applyResult.network });
        setRecords((current) =>
          current.map((candidate) =>
            candidate.target.deviceId === deviceId
              ? updateSerialSettings(candidate, { wifi_configured: false, wifi_ssid: null }, "wifi_config", message)
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
    if (!record.serial) return serialCommandUnavailable();
    const session = serialSessions.current.get(deviceId);
    if (!session) return serialCommandUnavailable();
    try {
      onProgress?.({ phase: "clearing", message: "Clearing WiFi credentials from hardware" });
      await session.clearWifiConfig();
      const status = await waitForSerialWifiDisabled(session, onProgress);
      const message = wifiDisabledMessage(status.network);
      onProgress?.({ phase: "disabled", message, network: status.network });
      setRecords((current) =>
        current.map((candidate) =>
          candidate.target.deviceId === deviceId
            ? updateSerialSettings(candidate, { wifi_configured: false, wifi_ssid: null }, "wifi_config", message)
            : candidate,
        ),
      );
      return { ok: true, message, network: status.network };
    } catch (error) {
      const envelope = errorFromSerialFailure(error);
      setSerialCommandError(deviceId, envelope);
      return { ok: false, error: envelope };
    }
  }, [records, setSerialCommandError]);

  const setSerialLogLevel = useCallback(async (deviceId: string, level: SafeSettingsState["log_level"]): Promise<CommandResult> => {
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    if (!record) return serialCommandUnavailable();
    const devdBaseUrl = devdBaseUrlForRecord(record);
    if (devdBaseUrl !== null) {
      try {
        const leaseId = devdLeaseIdForRecord(record);
        if (!leaseId) return serialCommandUnavailable();
        await setDevdLogLevel(devdBaseUrl, record.target.deviceId, leaseId, level);
        await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
      } catch (error) {
        const envelope = toErrorEnvelope(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    } else if (!record.target.mock) {
      if (!record.serial) return serialCommandUnavailable();
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
    if (!record) return serialCommandUnavailable();
    const devdBaseUrl = devdBaseUrlForRecord(record);
    if (devdBaseUrl !== null) {
      try {
        const leaseId = devdLeaseIdForRecord(record);
        if (!leaseId) return serialCommandUnavailable();
        await setDevdManualChargePrefs(devdBaseUrl, record.target.deviceId, leaseId, prefs);
        await updateDevdSerialSnapshot(record.target.deviceId, devdBaseUrl);
      } catch (error) {
        const envelope = toErrorEnvelope(error);
        setSerialCommandError(deviceId, envelope);
        return { ok: false, error: envelope };
      }
    } else if (!record.target.mock) {
      if (!record.serial) return serialCommandUnavailable();
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

  function appendSerialStatusToSession(status: SerialStatusFrame["status"], deviceId: string | null) {
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
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    const leaseId = record?.serial?.leaseId;
    if (!leaseId) throw new Error("devd Web lease is missing");
    const session = await getDevdSerialSession(baseUrl, { ...DEVD_SERIAL_SESSION_LIMITS, leaseId });
    setRecords((current) =>
      current.map((record) => (record.target.deviceId === deviceId ? mergeDevdSerial(record, baseUrl, session, {
        lease_id: leaseId,
        expires_at: record.serial?.leaseExpiresAt ?? "",
        heartbeat_interval_ms: record.serial?.heartbeatIntervalMs ?? 2000,
        lease_ttl_ms: record.serial?.leaseTtlMs ?? 8000,
      }) : record)),
    );
  }

  function startDevdLeaseHeartbeat(record: DeviceRecord) {
    const leaseId = record.serial?.leaseId;
    const baseUrl = record.serial?.baseUrl ?? "";
    if (!leaseId) return;
    const existing = devdLeaseHeartbeats.current.get(record.target.deviceId);
    if (existing !== undefined) window.clearInterval(existing);
    const intervalMs = Math.max(1000, Math.min(record.serial?.heartbeatIntervalMs ?? 2000, 5000));
    const heartbeat = window.setInterval(() => {
      void heartbeatDevdWebLease(baseUrl, leaseId)
        .then((lease) => {
          setRecords((current) =>
            current.map((candidate) =>
              candidate.target.deviceId === record.target.deviceId && candidate.serial?.leaseId === leaseId
                ? {
                    ...candidate,
                    streamState: candidate.streamState === "error" ? "polling" : candidate.streamState,
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
              candidate.target.deviceId === record.target.deviceId && candidate.serial?.leaseId === leaseId
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

  const removeDevice = useCallback((deviceId: string) => {
    const record = records.find((candidate) => candidate.target.deviceId === deviceId);
    streams.current.get(deviceId)?.close();
    streams.current.delete(deviceId);
    devdStreams.current.get(deviceId)?.close();
    devdStreams.current.delete(deviceId);
    const heartbeat = devdLeaseHeartbeats.current.get(deviceId);
    if (heartbeat !== undefined) window.clearInterval(heartbeat);
    devdLeaseHeartbeats.current.delete(deviceId);
    void serialSessions.current.get(deviceId)?.close();
    serialSessions.current.delete(deviceId);
    setRecords((current) => current.filter((record) => record.target.deviceId !== deviceId));
    if (record?.serial?.source === "devd") {
      void disconnectDevdSerialDevice(record).catch((error) => {
        console.warn("failed to disconnect devd device", error);
      });
    }
  }, [records]);

  const resetDemo = useCallback(() => {
    for (const stream of streams.current.values()) stream.close();
    streams.current.clear();
    for (const stream of devdStreams.current.values()) stream.close();
    devdStreams.current.clear();
    for (const heartbeat of devdLeaseHeartbeats.current.values()) window.clearInterval(heartbeat);
    devdLeaseHeartbeats.current.clear();
    for (const record of records) releaseDevdLeaseForRecord(record, true);
    for (const session of serialSessions.current.values()) void session.close();
    serialSessions.current.clear();
    setRecords(makeMockRecords("default"));
  }, []);

  const value = useMemo(
    () => ({
      records,
      addDevice,
      addDevdDevice,
      connectUsbSerialDevice,
      prepareWebSerialFlashPort,
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
      addDevdDevice,
      connectUsbSerialDevice,
      prepareWebSerialFlashPort,
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

function getDemoSeed(): DemoSeed | null {
  const seed = new URLSearchParams(window.location.search).get("seed");
  return isDemoSeed(seed) ? seed : null;
}

function loadInitialRecords(seed: DemoSeed | null): DeviceRecord[] {
  if (seed) return makeMockRecords(seed);

  const stored = localStorage.getItem(STORAGE_KEY);
  if (!stored) return [];

  try {
    type StoredDeviceTarget = Omit<DeviceTarget, "transport"> & { transport?: DeviceTarget["transport"] | string };
    const targets = JSON.parse(stored) as StoredDeviceTarget[];
    if (!Array.isArray(targets)) return [];
    return targets
      .filter((target) => !target.mock)
      .flatMap((target) => {
        const normalizedTarget = normalizeStoredTarget(target);
        return normalizedTarget ? [recordFromStoredTarget(normalizedTarget)] : [];
      })
      .reduce((merged, record) => upsertRecord(merged, record), [] as DeviceRecord[]);
  } catch {
    return [];
  }
}

function persistedTargetsForRecord(record: DeviceRecord): DeviceTarget[] {
  if (record.target.mock || record.target.transport === "serial") return [];
  const targets = [record.target];
  if (record.serial?.source === "devd" && record.serial.baseUrl && record.target.transport !== "devd") {
    targets.push({
      deviceId: record.target.deviceId,
      baseUrl: record.serial.baseUrl,
      alias: record.target.alias,
      location: record.target.location || "devd",
      addedAt: record.target.addedAt,
      bridgeAuth: record.target.bridgeAuth,
      transport: "devd",
      serialProtocol: record.serial.protocol ?? record.target.serialProtocol,
    });
  }
  return targets;
}

function recordFromStoredTarget(target: DeviceTarget): DeviceRecord {
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
    serial:
      target.transport === "devd"
        ? {
            connected: false,
            source: "devd",
            baseUrl: target.baseUrl,
            protocol: target.serialProtocol ?? "mains-aegis.cdc.v1",
            logs: [],
            trace: [],
            safeSettings: defaultSafeSettings(),
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
    status: result.status,
    connectionState,
    streamState,
    error: null,
    lastUpdated: new Date().toISOString(),
  };
}

function normalizeStoredTarget(target: Omit<DeviceTarget, "transport"> & { transport?: DeviceTarget["transport"] | string }): DeviceTarget | null {
  if (target.transport === LEGACY_DEVD_TRANSPORT) {
    return { ...target, transport: "devd", location: target.location || "devd" };
  }
  if (target.transport === undefined || target.transport === "http" || target.transport === "serial" || target.transport === "devd") {
    return target as DeviceTarget;
  }
  return null;
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
      source: target.mock ? "mock" : "web_serial",
      protocol,
      logs,
      trace,
      safeSettings: defaultSafeSettings(),
    },
  };
}

type DevdLeaseSnapshot = Pick<DevdWebLease, "lease_id" | "expires_at" | "heartbeat_interval_ms" | "lease_ttl_ms">;

function recordFromDevdProbe(target: DeviceTarget, result: ProbeResult, session: DevdSerialSession, lease: DevdLeaseSnapshot): DeviceRecord {
  return mergeDevdSerial(recordFromProbe(target, result, "online", "polling"), target.baseUrl, session, lease);
}

function mergeDevdSerial(record: DeviceRecord, baseUrl: string, session: DevdSerialSession, lease?: DevdLeaseSnapshot): DeviceRecord {
  return {
    ...record,
    target: {
      ...record.target,
      serialProtocol: session.protocol,
    },
    connectionState: session.connected ? "online" : record.connectionState,
    error: session.connected ? null : record.error,
    lastUpdated: new Date().toISOString(),
    status: session.status ?? record.status,
    network: record.network && session.status
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
      heartbeatIntervalMs: lease?.heartbeat_interval_ms ?? record.serial?.heartbeatIntervalMs,
      leaseTtlMs: lease?.lease_ttl_ms ?? record.serial?.leaseTtlMs,
      protocol: session.protocol,
      status: session.status ?? null,
      logs: session.logs,
      trace: session.trace,
      safeSettings: session.safeSettings,
    },
  };
}

async function disconnectDevdSerialDevice(record: DeviceRecord): Promise<void> {
  const baseUrl = record.serial?.baseUrl ?? "";
  if (record.serial?.leaseId) {
    await releaseDevdWebLease(baseUrl, record.serial.leaseId);
    return;
  }
  const devices = await listDevdDevices(baseUrl);
  const devdDevice = devices.devices.find((device) => device.identity?.device_id === record.target.deviceId);
  if (!devdDevice) return;
  await disconnectDevdDevice(devdDevice.id, baseUrl);
}

function releaseDevdLeaseForRecord(record: DeviceRecord, keepalive = false) {
  if (record.serial?.source !== "devd" || !record.serial.leaseId) return;
  void releaseDevdWebLease(record.serial.baseUrl ?? "", record.serial.leaseId, keepalive).catch(() => undefined);
}

function upsertRecord(records: DeviceRecord[], record: DeviceRecord): DeviceRecord[] {
  const existing = records.find((candidate) => candidate.target.deviceId === record.target.deviceId);
  const next = records.filter((candidate) => candidate.target.deviceId !== record.target.deviceId);
  next.push(existing ? mergeDeviceRecord(existing, record) : record);
  return next;
}

function mergeDeviceRecord(existing: DeviceRecord, incoming: DeviceRecord): DeviceRecord {
  const existingTransport = existing.target.transport ?? "http";
  const incomingTransport = incoming.target.transport ?? "http";
  const preferIncomingTarget = incomingTransport === "http" || existingTransport !== "http";
  return {
    ...existing,
    ...incoming,
    target: preferIncomingTarget
      ? {
          ...incoming.target,
          alias: incoming.target.alias || existing.target.alias,
          location: incoming.target.location || existing.target.location,
        }
      : {
          ...existing.target,
          serialProtocol: incoming.target.serialProtocol ?? existing.target.serialProtocol,
        },
    serial: incoming.serial ?? existing.serial,
    status: incoming.status ?? existing.status,
    network: incoming.network ?? existing.network,
    identity: incoming.identity ?? existing.identity,
    connectionState: incoming.connectionState === "online" || existing.connectionState === "online" ? "online" : incoming.connectionState,
    streamState: incoming.streamState === "streaming" || existing.streamState === "streaming" ? "streaming" : incoming.streamState,
    error: incoming.error ?? existing.error,
    lastUpdated: incoming.lastUpdated ?? existing.lastUpdated,
  };
}

function isDevdSerial(record: DeviceRecord): record is DeviceRecord & { serial: NonNullable<DeviceRecord["serial"]> & { source: "devd"; baseUrl: string } } {
  return record.serial?.source === "devd" && Boolean(record.serial.baseUrl);
}

function devdBaseUrlForRecord(record: DeviceRecord): string | null {
  if (record.serial?.source === "devd") return record.serial.baseUrl ?? "";
  if (record.target.transport === "devd") return record.target.baseUrl ?? "";
  return null;
}

function devdLeaseIdForRecord(record: DeviceRecord): string | null {
  return record.serial?.source === "devd" ? record.serial.leaseId ?? null : null;
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
      trace: [...record.serial.trace, entry].slice(-DEVD_SERIAL_SESSION_LIMITS.traceLimit),
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
      throw new Error(`wifi_connect_failed: ${status.network.last_error ?? "unknown"}`);
    }
    await new Promise((resolve) => window.setTimeout(resolve, 750));
  }
  throw new Error(`wifi_${expectedState}_timeout: last network state ${JSON.stringify(lastNetwork)}`);
}

function wifiProgressFromNetwork(
  network: UpsStatus["network"],
  expectedState: UpsStatus["network"]["state"],
  ssid?: string,
): WifiProvisioningProgress {
  if (expectedState === "disabled") {
    return network.state === "disabled"
      ? { phase: "disabled", message: "WiFi credentials cleared and WiFi disconnected", network }
      : { phase: "clearing", message: "Disconnecting WiFi and clearing runtime credentials", network };
  }
  if (network.state === "connected") {
    return network.ipv4
      ? { phase: "connected", message: wifiConnectedMessage(ssid ?? "network", network), network }
      : { phase: "ip", message: "WiFi link is up. Waiting for an IP address", network };
  }
  if (network.state === "connecting") {
    return { phase: "connecting", message: ssid ? `Connecting to ${ssid} and waiting for an IP address` : "Connecting to WiFi and waiting for an IP address", network };
  }
  return { phase: "starting", message: "Starting WiFi with the saved credentials", network };
}

function wifiConnectedMessage(ssid: string, network: { state: string; ipv4: string | null }): string {
  return network.ipv4 ? `WiFi connected to ${ssid} at ${network.ipv4}` : `WiFi connected to ${ssid}`;
}

function wifiDisabledMessage(network: { state: string }): string {
  return network.state === "disabled" ? "WiFi credentials cleared and WiFi disconnected" : "WiFi credentials cleared";
}
