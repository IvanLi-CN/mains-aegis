import { ArrowRight, BadgeInfo, FileDown, Maximize2, Minimize2, WifiOff } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { flashDevdDevice, listDevdDevices, selectDevdArtifact } from "../api/client";
import type { DeviceRecord, DevdDevice, FirmwareArtifact } from "../api/types";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Checkbox } from "../components/ui/checkbox";
import { Label } from "../components/ui/label";
import { Progress } from "../components/ui/progress";
import { Separator } from "../components/ui/separator";
import { Sheet, SheetBody, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "../components/ui/sheet";
import { useDeviceRegistry } from "../device-registry/context";
import { firmwareArtifactHasWebFlashImages, firmwareArtifactImageFiles, firmwareArtifactMatchesIdentity, loadResolvedFirmwareCatalog, type ResolvedFirmwareArtifact } from "../firmware/catalog";
import { flashArtifactWithWebSerial, isWebSerialFlashSupported, type WebSerialFlashProgress } from "../firmware/webSerialFlasher";
import { timeAgo } from "../utils/format";
import { deviceSeverity } from "../utils/severity";

type FlashMethod = "web_serial" | "devd";
type FlashRunState = "idle" | "running" | "success" | "error";

type FlashUiProgress = WebSerialFlashProgress & {
  source: FlashMethod | "mock";
};

type FlashLogEntry = {
  id: string;
  level: "info" | "success" | "error";
  message: string;
};

export function FirmwarePage({ record }: { record: DeviceRecord }) {
  const registry = useDeviceRegistry();
  const [catalog, setCatalog] = useState<ResolvedFirmwareArtifact[]>([]);
  const [catalogMessage, setCatalogMessage] = useState("Loading firmware catalog");
  const [selectedArtifactId, setSelectedArtifactId] = useState<string>("");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [flashState, setFlashState] = useState<FlashRunState>("idle");
  const [progress, setProgress] = useState<FlashUiProgress>(emptyFlashProgress());
  const [message, setMessage] = useState<string | null>(null);
  const [flashLog, setFlashLog] = useState<FlashLogEntry[]>(() => [makeFlashLogEntry("info", "Ready. Flash has not started.")]);
  const [logExpanded, setLogExpanded] = useState(false);
  const [expandedLogMaxHeight, setExpandedLogMaxHeight] = useState<number | null>(null);
  const [currentDevdDevice, setCurrentDevdDevice] = useState<DevdDevice | null>(null);
  const [devdMessage, setDevdMessage] = useState<string | null>(null);
  const logPanelRef = useRef<HTMLDivElement | null>(null);

  const identity = record.identity;
  const method = resolveFlashMethod(record);
  const flashLocked = flashState === "running";
  const matchingArtifacts = useMemo(
    () => catalog.filter((entry) => identity && firmwareArtifactMatchesIdentity(entry.artifact, identity)),
    [catalog, identity],
  );
  const currentEntry = matchingArtifacts[0] ?? null;
  const selectedEntry = catalog.find((entry) => entry.artifact.artifact_id === selectedArtifactId) ?? catalog[0] ?? null;
  const selectedArtifact = selectedEntry?.artifact ?? null;
  const selectedImages = selectedArtifact ? firmwareArtifactImageFiles(selectedArtifact) : [];
  const webSerialAvailable = isWebSerialFlashSupported();
  const webSerialReady = method === "web_serial" && webSerialAvailable && Boolean(selectedArtifact && firmwareArtifactHasWebFlashImages(selectedArtifact));
  const devdArtifactLocal = selectedEntry?.source !== "github_release";
  const devdReady =
    method === "devd" &&
    devdArtifactLocal &&
    record.target.transport === "devd" &&
    Boolean(currentDevdDevice?.binding && currentDevdDevice.connection === "connected");
  const canFlash = confirmed && flashState !== "running" && (record.target.mock || webSerialReady || devdReady);
  const disableReason = flashDisableReason({
    method,
    selectedArtifact,
    webSerialAvailable,
    webSerialReady,
    devdReady,
    devdArtifactLocal,
    confirmed,
    flashState,
    record,
  });

  useEffect(() => {
    let cancelled = false;
    loadResolvedFirmwareCatalog()
      .then((resolution) => {
        if (cancelled) return;
        setCatalog(resolution.artifacts);
        const sourceParts = [
          `bundled ${resolution.source_status.bundled}`,
          `release ${resolution.source_status.github_release}`,
          `${resolution.overridden_release_count} release duplicate${resolution.overridden_release_count === 1 ? "" : "s"} overridden`,
        ];
        setCatalogMessage(sourceParts.join(", "));
      })
      .catch((error) => {
        if (!cancelled) setCatalogMessage(error instanceof Error ? error.message : "Firmware catalog unavailable");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const next = currentEntry?.artifact.artifact_id ?? catalog[0]?.artifact.artifact_id ?? "";
    if (!selectedArtifactId || !catalog.some((entry) => entry.artifact.artifact_id === selectedArtifactId)) {
      setSelectedArtifactId(next);
    }
  }, [catalog, currentEntry, selectedArtifactId]);

  useEffect(() => {
    if (method === "devd" && drawerOpen) {
      void resolveCurrentDevdDevice().catch(() => undefined);
    }
  }, [drawerOpen, method, record.target.baseUrl, record.target.deviceId]);

  useEffect(() => {
    setConfirmed(false);
    setMessage(null);
    setFlashState("idle");
    setProgress(emptyFlashProgress());
    setFlashLog([makeFlashLogEntry("info", "Ready. Flash has not started.")]);
    setLogExpanded(false);
  }, [selectedArtifactId, method]);

  useEffect(() => {
    if (!flashLocked) return;
    const warnBeforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warnBeforeUnload);
    return () => window.removeEventListener("beforeunload", warnBeforeUnload);
  }, [flashLocked]);

  useEffect(() => {
    if (!logExpanded) {
      setExpandedLogMaxHeight(null);
      return;
    }
    function updateExpandedLogHeight() {
      const panel = logPanelRef.current;
      if (!panel) return;
      const rect = panel.getBoundingClientRect();
      setExpandedLogMaxHeight(Math.max(180, window.innerHeight - rect.top - 28));
    }
    updateExpandedLogHeight();
    window.addEventListener("resize", updateExpandedLogHeight);
    return () => window.removeEventListener("resize", updateExpandedLogHeight);
  }, [logExpanded]);

  async function resolveCurrentDevdDevice(): Promise<DevdDevice | null> {
    if (record.target.transport !== "devd") return null;
    try {
      const result = await listDevdDevices(record.target.baseUrl);
      const match = result.devices.find((device) => device.identity?.device_id === record.target.deviceId) ?? null;
      setCurrentDevdDevice(match);
      setDevdMessage(match ? null : "No devd device identity matches the selected Firmware record. Reconnect the intended device from Connect.");
      return match;
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : "Unable to read the current devd adapter session.";
      setCurrentDevdDevice(null);
      setDevdMessage(errorMessage);
      return null;
    }
  }

  function openDrawer(entry: ResolvedFirmwareArtifact) {
    if (flashLocked) return;
    setSelectedArtifactId(entry.artifact.artifact_id);
    setDrawerOpen(true);
    setConfirmed(false);
    setMessage(null);
    setFlashState("idle");
    setProgress(emptyFlashProgress());
    setFlashLog([makeFlashLogEntry("info", "Ready. Flash has not started.")]);
    if (method === "devd") {
      void resolveCurrentDevdDevice().catch(() => undefined);
    }
  }

  async function onFlash() {
    if (!selectedArtifact) return;
    setFlashState("running");
    setMessage(null);
    setFlashLog([makeFlashLogEntry("info", `Starting ${method === "devd" ? "devd adapter" : "Web Serial"} flash for ${selectedArtifact.artifact_id}.`)]);
    if (record.target.mock) {
      await runMockFlash((next) => {
        setProgress(next);
        setFlashLog((current) => appendFlashLog(current, "info", next.message));
      });
      setFlashState("success");
      setMessage("Mock flash completed");
      setFlashLog((current) => appendFlashLog(current, "success", "Mock flash completed."));
      return;
    }
    try {
      if (method === "web_serial") {
        const port = await registry.prepareWebSerialFlashPort(record.target.deviceId);
        if (!port) {
          throw new Error("The current Web Serial connection is not available. Connect USB before flashing.");
        }
        await flashArtifactWithWebSerial({
          artifact: selectedArtifact,
          artifactMatch: selectedEntry,
          port,
          onProgress: (next) => {
            setProgress({ ...next, source: "web_serial" });
            setFlashLog((current) => appendFlashLog(current, "info", next.message));
          },
        });
      } else {
        if (!devdArtifactLocal) {
          throw new Error("devd flashing requires a bundled artifact because the daemon reads firmware files from local disk.");
        }
        const currentDevice = currentDevdDevice ?? (await resolveCurrentDevdDevice());
        if (!currentDevice?.binding) {
          throw new Error("Open Connect, bind the devd device, then return here to flash.");
        }
        setProgress(makeFlashUiProgress("devd", "verify", "Selecting devd artifact", 12, 0, 0));
        setFlashLog((current) => appendFlashLog(current, "info", "Selecting devd artifact."));
        await selectDevdArtifact(
          currentDevice.id,
          {
            artifact: selectedArtifact,
            artifact_id: selectedArtifact.artifact_id,
            ...(selectedEntry?.manifest_path ? { manifest_path: selectedEntry.manifest_path } : {}),
          },
          record.target.baseUrl,
        );
        setProgress(makeFlashUiProgress("devd", "verify", "Running dry-run", 36, 0, 0));
        setFlashLog((current) => appendFlashLog(current, "info", "Running devd dry-run."));
        await flashDevdDevice(currentDevice.id, { artifact_id: selectedArtifact.artifact_id, dry_run: true }, record.target.baseUrl);
        setProgress(makeFlashUiProgress("devd", "write", "devd flash running", 58, 0, 0));
        setFlashLog((current) => appendFlashLog(current, "info", "Writing firmware through devd."));
        await flashDevdDevice(currentDevice.id, { artifact_id: selectedArtifact.artifact_id }, record.target.baseUrl);
        setProgress(makeFlashUiProgress("devd", "done", "devd flash completed", 100, 0, 0));
        setFlashLog((current) => appendFlashLog(current, "success", "devd flash completed."));
      }
      setFlashState("success");
      setMessage("Flash completed. Reconnect the device if the USB CDC session was reset.");
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : "Firmware flash failed";
      setFlashState("error");
      setProgress(makeFlashUiProgress(method, "error", errorMessage, progress.percent, progress.written, progress.total));
      setMessage(errorMessage);
      setFlashLog((current) => appendFlashLog(current, "error", errorMessage));
    }
  }

  const currentArtifact = currentEntry?.artifact ?? null;
  const catalogStats = useMemo(
    () => ({
      total: catalog.length,
      bundled: catalog.filter((entry) => entry.source !== "github_release").length,
      release: catalog.filter((entry) => entry.source === "github_release").length,
      readyForWebSerial: catalog.filter((entry) => firmwareArtifactHasWebFlashImages(entry.artifact)).length,
      overridden: catalog.filter((entry) => entry.source === "bundled_overrides_release").length,
    }),
    [catalog],
  );

  function onDrawerKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (event.key !== "Enter" || !canFlash) return;
    const target = event.target as HTMLElement | null;
    if (target?.tagName === "TEXTAREA") return;
    event.preventDefault();
    void onFlash();
  }

  function onDrawerOpenChange(open: boolean) {
    if (flashLocked && !open) return;
    setDrawerOpen(open);
  }

  function preventDrawerDismissWhileFlashing(event: Event) {
    if (!flashLocked) return;
    event.preventDefault();
  }

  return (
    <section className="page-flow firmware-page" data-evidence-target="firmware-flash">
      <div className="firmware-hero">
        <div className="firmware-hero-copy">
          <span className="eyebrow">Firmware</span>
          <h2>Version library and flash drawer</h2>
          <p>Current build, source-aware version list, and a drawer-based flash workflow that follows the active connection.</p>
        </div>

        <div className="firmware-hero-summary-grid">
          <Card className="firmware-summary-card">
            <CardHeader>
              <CardTitle>Current version</CardTitle>
              <CardDescription>{identity?.hostname ?? record.target.deviceId}</CardDescription>
            </CardHeader>
            <CardContent className="firmware-summary-body">
              <MetricRow label="Package" value={identity?.firmware.package_version ?? "--"} />
              <MetricRow label="Build" value={identity?.firmware.build_id ?? "--"} />
              <MetricRow label="Profile" value={identity?.firmware.build_profile ?? "--"} />
              <MetricRow label="Git" value={identity?.firmware.git_sha ?? "--"} />
              <MetricRow label="Matched artifact" value={currentArtifact?.artifact_id ?? "No catalog match"} />
            </CardContent>
          </Card>

          <Card className="firmware-summary-card">
            <CardHeader>
              <CardTitle>Catalog health</CardTitle>
              <CardDescription>{catalogMessage}</CardDescription>
            </CardHeader>
            <CardContent className="firmware-summary-body">
              <MetricRow label="Artifacts" value={String(catalogStats.total)} />
              <MetricRow label="Bundled" value={String(catalogStats.bundled)} />
              <MetricRow label="Releases" value={String(catalogStats.release)} />
              <MetricRow label="Web Serial ready" value={String(catalogStats.readyForWebSerial)} />
              <MetricRow label="Overridden" value={String(catalogStats.overridden)} />
            </CardContent>
          </Card>
        </div>
      </div>

      <Card className="firmware-list-card">
        <CardHeader>
          <div className="firmware-list-header">
            <div>
              <CardTitle>Version list</CardTitle>
              <CardDescription>Each version can be opened in the drawer and flashed from there.</CardDescription>
            </div>
            <Badge className="ui-badge-muted">{catalog.length} versions</Badge>
          </div>
        </CardHeader>
        <CardContent className="firmware-version-list">
          {catalog.length > 0 ? (
            catalog.map((entry) => (
              <FirmwareVersionCard
                key={entry.artifact.artifact_id}
                entry={entry}
                currentEntry={currentArtifact?.artifact_id === entry.artifact.artifact_id}
                webSerialAvailable={firmwareArtifactHasWebFlashImages(entry.artifact)}
                onFlash={() => openDrawer(entry)}
              />
            ))
          ) : (
            <div className="firmware-empty">
              <Badge className="ui-badge-muted">Empty</Badge>
              <p>No firmware artifacts available.</p>
            </div>
          )}
        </CardContent>
      </Card>

      <Sheet open={drawerOpen} onOpenChange={onDrawerOpenChange}>
        <SheetContent
          className="firmware-drawer"
          closeDisabled={flashLocked}
          onEscapeKeyDown={preventDrawerDismissWhileFlashing}
          onPointerDownOutside={preventDrawerDismissWhileFlashing}
          onInteractOutside={preventDrawerDismissWhileFlashing}
        >
          <SheetHeader>
            <SheetTitle>Flash firmware</SheetTitle>
            <SheetDescription>
              {selectedArtifact ? `${selectedArtifact.version} · ${selectedArtifact.profile} · ${formatSourceLabel(selectedEntry?.source ?? "github_release")}` : "Choose a firmware version from the list."}
            </SheetDescription>
          </SheetHeader>

          <SheetBody className="firmware-drawer-body" onKeyDown={onDrawerKeyDown}>
            {selectedArtifact ? (
              <div className="firmware-drawer-stack">
                <Card className="firmware-detail-section firmware-flash-section">
                  <CardHeader className="firmware-flash-header">
                    <CardTitle>Flash information</CardTitle>
                    <Badge className={flashState === "success" ? "ui-badge-ok" : flashState === "error" ? "ui-badge-warning" : "ui-badge-muted"}>{flashState}</Badge>
                  </CardHeader>
                  <CardContent className="firmware-section-content">
                    <div className="firmware-flash-layout">
                      <div className="firmware-flash-primary">
                        <div className="firmware-flash-topline">
                          <div>
                            <strong>{Math.round(progress.percent)}%</strong>
                            <span>{progress.message}</span>
                          </div>
                        </div>
                        <Progress value={progress.percent} />
                        <div className="firmware-flash-metrics">
                          <MetricRow label="Stage" value={progress.stage} />
                          <MetricRow label="Task" value={flashState} />
                          <MetricRow label="Written" value={formatByteCount(progress.written)} />
                          <MetricRow label="Total" value={formatByteCount(progress.total)} />
                          <MetricRow label="Path" value={method === "devd" ? "devd adapter" : "Web Serial"} />
                          <MetricRow label="Outcome" value={message ?? "No run"} />
                        </div>
                      </div>
                      <div className="firmware-flash-side">
                        <div className="firmware-flash-action-panel">
                          <div className="firmware-flash-action-body">
                            <Label htmlFor="firmware-confirm" className="firmware-confirm-row">
                              <Checkbox id="firmware-confirm" checked={confirmed} disabled={flashLocked} onCheckedChange={(checked) => setConfirmed(checked === true)} />
                              <span>
                                I have selected the intended ESP32-S3 device and firmware version.
                              </span>
                            </Label>
                            <Button type="button" size="sm" disabled={!canFlash || flashLocked} onClick={() => void onFlash()}>
                              <FileDown size={15} />
                              {flashState === "running" ? "Flashing" : flashState === "success" ? "Flash again" : "Start flash"}
                            </Button>
                          </div>
                          {disableReason ? <p className="firmware-help">{disableReason}</p> : <p className="firmware-help">Press Enter to start, or use the button.</p>}
                        </div>
                      </div>
                      <div
                        ref={logPanelRef}
                        className={`firmware-log-panel ${logExpanded ? "is-expanded" : ""}`}
                        style={expandedLogMaxHeight ? { "--firmware-log-expanded-max-height": `${expandedLogMaxHeight}px` } as React.CSSProperties : undefined}
                      >
                        <div className="firmware-log-header">
                          <span className="eyebrow">Stage log</span>
                          <div className="firmware-log-actions">
                            <Badge className="ui-badge-muted">{flashLog.length} entries</Badge>
                            <Button type="button" variant="ghost" size="sm" onClick={() => setLogExpanded((expanded) => !expanded)}>
                              {logExpanded ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
                              {logExpanded ? "Collapse" : "Expand"}
                            </Button>
                          </div>
                        </div>
                        <div className="firmware-log-lines" role="log" aria-live="polite">
                          {flashLog.map((entry) => (
                            <div key={entry.id} className={`firmware-log-line is-${entry.level}`}>
                              <span>{entry.level}</span>
                              <p>{entry.message}</p>
                            </div>
                          ))}
                        </div>
                      </div>
                    </div>
                  </CardContent>
                </Card>

                <div className="firmware-support-panel">
                  <section className="firmware-context-section" aria-labelledby="firmware-info-title">
                    <header>
                      <h3 id="firmware-info-title">Firmware information</h3>
                      <p>Selected artifact and flashable image metadata.</p>
                    </header>
                    <div className="firmware-section-content">
                      <div className="firmware-dashboard-head">
                        <div>
                          <span>Version</span>
                          <strong>{selectedArtifact.version}</strong>
                        </div>
                        <div className="firmware-badge-row">
                          <SourceBadge source={selectedEntry?.source ?? "github_release"} />
                          {currentArtifact?.artifact_id === selectedArtifact.artifact_id ? <Badge className="ui-badge-current">Current</Badge> : null}
                          <Badge className={selectedImages.length > 0 ? "ui-badge-ok" : "ui-badge-muted"}>{selectedImages.length > 0 ? "Web Serial ready" : "Web Serial unavailable"}</Badge>
                        </div>
                      </div>
                      <div className="firmware-stat-strip">
                        <StatusTile label="Profile" value={selectedArtifact.profile} />
                        <StatusTile label="Files" value={`${selectedArtifact.files.length}`} />
                        <StatusTile label="Web images" value={`${selectedImages.length}`} />
                      </div>
                      <div className="firmware-identity-list">
                        <IdentityRow label="Artifact" value={selectedArtifact.artifact_id} />
                        <IdentityRow label="Build" value={selectedArtifact.build_id} />
                      </div>
                      {selectedImages.length > 0 ? (
                        <>
                          <Separator />
                          <div className="firmware-image-list">
                            {selectedImages.map((file) => (
                              <div key={file.path} className="firmware-image-row">
                                <div>
                                  <strong>{file.path}</strong>
                                  <span>{formatByteCount(file.size)}</span>
                                </div>
                                <Badge className="ui-badge-muted">0x{file.flash_address.toString(16)}</Badge>
                              </div>
                            ))}
                          </div>
                        </>
                      ) : null}
                    </div>
                  </section>

                  <section className="firmware-context-section" aria-labelledby="hardware-info-title">
                    <header>
                      <h3 id="hardware-info-title">Hardware information</h3>
                      <p>Current device context; the flash path is inherited from this connection.</p>
                    </header>
                    <div className="firmware-section-content">
                      <div className="firmware-dashboard-head">
                        <div>
                          <span>Path</span>
                          <strong>{method === "devd" ? "devd adapter" : "Web Serial"}</strong>
                        </div>
                        <Badge className={currentDevdDevice?.binding || method === "web_serial" ? "ui-badge-ok" : "ui-badge-muted"}>
                          {method === "devd" ? (currentDevdDevice?.binding ? "bound" : "not bound") : webSerialAvailable ? "browser ready" : "unsupported"}
                        </Badge>
                      </div>
                      <div className="firmware-stat-strip">
                        <StatusTile label="State" value={record.status ? `${record.status.mode} · ${deviceSeverity(record)}` : "--"} />
                        <StatusTile label="Location" value={record.target.location} />
                        <StatusTile label="Updated" value={timeAgo(record.lastUpdated)} />
                        {method === "devd" ? (
                          <>
                            <StatusTile label="Session" value={currentDevdDevice?.connection === "connected" ? "connected" : "not connected"} />
                            <StatusTile label="Binding" value={currentDevdDevice?.binding ? "bound" : "resolved"} />
                            <StatusTile label="Artifact files" value={devdArtifactLocal ? "local" : "release only"} />
                          </>
                        ) : (
                          <>
                            <StatusTile label="Browser support" value={webSerialAvailable ? "available" : "unsupported"} />
                            <StatusTile label="Images" value={selectedImages.length > 0 ? `${selectedImages.length}` : "none"} />
                          </>
                        )}
                      </div>
                      <div className="firmware-identity-list">
                        <IdentityRow label="Hostname" value={identity?.hostname ?? record.target.deviceId} />
                        {method === "devd" ? <IdentityRow label="Adapter" value={record.target.baseUrl || "same-origin"} /> : null}
                      </div>
                      {devdMessage ? (
                        <div className="firmware-empty-note">
                          <BadgeInfo size={16} />
                          <p>{devdMessage}</p>
                        </div>
                      ) : null}
                      {method === "web_serial" && selectedImages.length === 0 ? (
                        <div className="firmware-empty-note">
                          <WifiOff size={16} />
                          <p>Choose a Web-flash image artifact, or connect through devd to flash an ELF.</p>
                        </div>
                      ) : null}
                    </div>
                  </section>
                </div>
              </div>
            ) : null}
          </SheetBody>
        </SheetContent>
      </Sheet>
    </section>
  );
}

function FirmwareVersionCard({
  entry,
  currentEntry,
  webSerialAvailable,
  onFlash,
}: {
  entry: ResolvedFirmwareArtifact;
  currentEntry: boolean;
  webSerialAvailable: boolean;
  onFlash: () => void;
}) {
  const images = firmwareArtifactImageFiles(entry.artifact);
  return (
    <Card className={`firmware-version-card ${currentEntry ? "is-current" : ""}`}>
      <CardContent className="firmware-version-card-content">
        <div className="firmware-version-main">
          <div className="firmware-version-title-row">
            <div>
              <CardTitle>{entry.artifact.version}</CardTitle>
              <CardDescription>{entry.artifact.profile}</CardDescription>
              <code className="firmware-artifact-id">{entry.artifact.artifact_id}</code>
            </div>
            <div className="firmware-badge-row">
              <SourceBadge source={entry.source} />
              {currentEntry ? <Badge className="ui-badge-current">Current</Badge> : null}
            </div>
          </div>

          <div className="firmware-version-meta">
            <MetricRow label="Profile" value={entry.artifact.profile} />
            <MetricRow label="Build" value={entry.artifact.build_id} />
            <MetricRow label="Files" value={`${entry.artifact.files.length}`} />
            <MetricRow label="Web images" value={`${images.length}`} />
          </div>

          <Separator />

          <div className="firmware-version-footer">
            <div className="firmware-badge-row">
              <Badge className={webSerialAvailable ? "ui-badge-ok" : "ui-badge-muted"}>{webSerialAvailable ? "Web Serial ready" : "Web Serial unavailable"}</Badge>
              <Badge className="ui-badge-muted">Current path decides flashing</Badge>
            </div>
            <div className="firmware-version-actions">
              <Button variant="outline" onClick={onFlash}>
                Flash
                <ArrowRight size={15} />
              </Button>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function SourceBadge({ source }: { source: "bundled" | "github_release" | "bundled_overrides_release" }) {
  const label = formatSourceLabel(source);
  const className = source === "github_release" ? "ui-badge-info" : source === "bundled_overrides_release" ? "ui-badge-warning" : "ui-badge-ok";
  return <Badge className={className}>{label}</Badge>;
}

function formatSourceLabel(source: "bundled" | "github_release" | "bundled_overrides_release") {
  return source === "github_release" ? "GitHub Release" : source === "bundled_overrides_release" ? "Bundled override" : "Bundled";
}

function CodeValue({ children, truncate = false }: { children: ReactNode; truncate?: boolean }) {
  const title = typeof children === "string" ? children : undefined;
  return (
    <code className={`firmware-code-value ${truncate ? "is-truncated" : ""}`} title={title}>
      {children}
    </code>
  );
}

function StatusTile({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="firmware-status-tile">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function IdentityRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="firmware-identity-row">
      <span>{label}</span>
      <code title={value}>{value}</code>
    </div>
  );
}

function MetricRow({ label, value, wide = false }: { label: string; value: ReactNode; wide?: boolean }) {
  return (
    <div className={`firmware-metric-row ${wide ? "is-wide" : ""}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function resolveFlashMethod(record: DeviceRecord): FlashMethod {
  return record.target.transport === "devd" ? "devd" : "web_serial";
}

function emptyFlashProgress(): FlashUiProgress {
  return {
    stage: "idle",
    message: "Not started yet",
    percent: 0,
    written: 0,
    total: 0,
    fileIndex: null,
    source: "mock",
  };
}

function makeFlashUiProgress(source: FlashMethod | "mock", stage: WebSerialFlashProgress["stage"], message: string, percent: number, written: number, total: number): FlashUiProgress {
  return {
    source,
    stage,
    message,
    percent,
    written,
    total,
    fileIndex: null,
  };
}

function makeFlashLogEntry(level: FlashLogEntry["level"], message: string): FlashLogEntry {
  return {
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    level,
    message,
  };
}

function appendFlashLog(current: FlashLogEntry[], level: FlashLogEntry["level"], message: string): FlashLogEntry[] {
  const last = current[current.length - 1];
  if (last?.level === level && last.message === message) return current;
  return [...current, makeFlashLogEntry(level, message)].slice(-8);
}

function flashDisableReason(input: {
  method: FlashMethod;
  selectedArtifact: FirmwareArtifact | null;
  webSerialAvailable: boolean;
  webSerialReady: boolean;
  devdReady: boolean;
  devdArtifactLocal: boolean;
  confirmed: boolean;
  flashState: FlashRunState;
  record: DeviceRecord;
}): string | null {
  if (!input.selectedArtifact) return "Choose a firmware version from the list.";
  if (!input.confirmed) return "Confirm this device and firmware to enable flashing.";
  if (input.flashState === "running") return "Flash is running. Keep this tab open; controls and drawer close are locked until it finishes.";
  if (input.method === "web_serial") {
    if (!input.webSerialAvailable) return "Use Chrome or Edge with Web Serial, or reconnect through devd.";
    if (!firmwareArtifactHasWebFlashImages(input.selectedArtifact)) return "Choose a Web-flash image artifact, or connect through devd to flash an ELF.";
    return null;
  }
  if (input.record.target.transport !== "devd") return "Connect this device through the devd adapter before flashing.";
  if (!input.devdArtifactLocal) return "devd can flash only bundled artifacts with local files. Use Web Serial for this GitHub Release artifact.";
  if (!input.devdReady) return "Open Connect, bind the devd device, then return to Firmware.";
  return null;
}

async function runMockFlash(onProgress: (progress: FlashUiProgress) => void): Promise<void> {
  onProgress(makeFlashUiProgress("mock", "fetch", "Fetching mock firmware", 10, 512 * 1024, 1024 * 1024));
  await wait(180);
  onProgress(makeFlashUiProgress("mock", "write", "Writing mock firmware", 62, 640 * 1024, 1024 * 1024));
  await wait(320);
  onProgress(makeFlashUiProgress("mock", "reset", "Resetting mock device", 94, 1024 * 1024, 1024 * 1024));
  await wait(120);
  onProgress(makeFlashUiProgress("mock", "done", "Mock flash completed", 100, 1024 * 1024, 1024 * 1024));
}

function wait(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function formatByteCount(value: number): string {
  if (!value) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  let next = value;
  while (next >= 1024 && index < units.length - 1) {
    next /= 1024;
    index += 1;
  }
  return `${next.toFixed(next >= 100 || index === 0 ? 0 : 1)} ${units[index]}`;
}
