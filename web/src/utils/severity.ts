import type { DeviceRecord, UpsStatus } from "../api/types";

export type Severity = "critical" | "warning" | "info" | "ok" | "offline";

export function deviceSeverity(record: DeviceRecord): Severity {
  if (record.connectionState === "offline") return "offline";
  if (record.connectionState === "error" || record.status?.mode === "fault") return "critical";
  if (!record.status) return "info";
  if (record.status.battery.no_battery || !record.status.battery.discharge_ready) return "critical";
  if (record.status.thermal.tmp_a_state === "hot" || record.status.thermal.tmp_b_state === "hot") return "critical";
  if (record.status.battery.state !== "ok" || (record.status.battery.soc_pct ?? 100) < 25) return "warning";
  if (record.status.output.gate_reason && record.status.output.gate_reason !== "none") return "warning";
  if (record.status.mode === "backup" || record.status.mode === "assist") return "info";
  return "ok";
}

export function severityRank(severity: Severity): number {
  return { critical: 0, warning: 1, info: 2, ok: 3, offline: 4 }[severity];
}

export function modeLabel(mode: UpsStatus["mode"] | undefined): string {
  if (!mode) return "UNKNOWN";
  return mode.toUpperCase();
}
