# Implementation

Status: implemented in firmware health eligibility; hardware lifecycle evidence remains pending.

The front-panel eligibility gate now treats online battery-present BQ40 `WARN` conditions such as RCA, cell undervoltage, XCHG and CHG_FET_OFF as Dashboard-compatible degraded states. BQ40 communication loss, no-battery/physical-VBAT failures, discharge-path failures, critical output protection, and any non-empty requested/active output mismatch remain Fatal and keep the device on SELF CHECK/BLOCKED.

USB-C input health is evaluated by path availability for page gating. Instantaneous input power is diagnostic telemetry and must not be interpreted as the negotiated PD contract; the bound-device acceptance run still needs to record Source Capabilities, Selected PDO, negotiated voltage/current, input limit and measured load separately.

The existing route behavior now preserves the active Dashboard route during degraded updates. Startup and Fatal recovery may enter the Dashboard default route after a fresh eligibility pass. Host coverage is passing; bound-device evidence requires an owner-authorized candidate firmware installation before the devd monitor run.
