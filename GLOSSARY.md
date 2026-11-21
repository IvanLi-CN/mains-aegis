# Mains Aegis Power and BMS Recovery

This context covers the UPS power path, BQ40 protection states, and owner-initiated recovery actions. Web UI page vocabulary lives in `CONTEXT.md`.

## Language

**Low-Voltage Precharge**:
An owner-initiated, one-shot recovery action for a pack whose path is blocked by a raw BQ40 `CUV` or `CUVC` condition. It is explicit and bounded; it is not the automatic charge policy.
_Avoid_: precharge flow, manual precharge, UV recover

**UV Suspect**:
A session-level condition in which the pack has shown undervoltage or remains near the low-voltage boundary, so charging must stay conservative even if an instantaneous cell reading rebounds.
_Avoid_: low battery, weak pack

**Automatic Low-Voltage Recovery**:
The normal charge policy that continues a controlled low-current charge when the pack is in its low-voltage window. It runs without an owner action, may run while the regulated output is blocked, and is distinct from Low-Voltage Precharge.
_Avoid_: auto precharge, RECOV action

**Low-Voltage Maintenance Flow**:
The host-side procedure that uses temporary BQ40 tool firmware to inspect or repair the pack baseline and then restores the main firmware. It is not a device-side action.
_Avoid_: recovery flow, tool recovery

**Pack Baseline**:
The board-frozen BQ40 Data Flash contract for protection thresholds, recovery points, FET behavior, and calibration. The main firmware must not rewrite it during ordinary charging.
_Avoid_: battery config, DF defaults

**OperationStatus[CHG]**:
The BQ40 logical charge-FET state reported in OperationStatus. It is the gauge's requested/logical state, not proof that the AFE gate or the external charge path is closed.
_Avoid_: charge allowed, physical CHG FET, CHG on

**AFE CHG FET**:
The BQ40 AFE hardware charge-FET gate state. It is a separate observation plane from OperationStatus and from the charger's BAT node.
_Avoid_: CHG FET (unqualified), charge switch

**Charger BAT Node**:
The BQ25792 BAT-pin observation expressed as `vbat_present` and `vbat_adc_mv`. It proves what the charger sees at its BAT pin, not whether a pack is electrically present further upstream.
_Avoid_: battery present, battery absent, pack disconnected

**FD / RCA**:
BQ40 BatteryStatus gauge-state flags for fully discharged and remaining-capacity alarm. They are gauge state, not active protection trips by themselves.
_Avoid_: battery protection, UV protection, protection latch

**Runtime Mode**:
The owner-facing operating mode of the UPS itself. It is not the charger policy and not the output gate.
_Avoid_: mode, BLOCKED used as a generic charge state

**Charger Policy**:
The automatic charge decision and its state/notice. It is independent of Runtime Mode.
_Avoid_: policy, charge mode

**Output Gate**:
The reason the regulated output cannot be active. It is not a charger-policy decision and not a statement that the battery cannot be charged.
_Avoid_: blocked, output blocked as a charge block

**Charge Path**:
The series path from the charger to the cells, including the BQ25792 BATFET/BAT node and the BQ40 CHG FET. It is either released or unproven; it is not the same as the discharge path.
_Avoid_: battery path, charge switch

**Backfeed Contradiction**:
A disagreement where the BMS reports the output or charge path open, or the charger is not charging, while the charger BAT node still reports voltage. It suggests reverse feed and is not the same as a low charger BAT node.
_Avoid_: VBAT absent, battery absent, VBAT present

**Hard Low-Voltage Floor**:
The cell voltage below which automatic charging is not allowed regardless of recovery policy. It is not the BQ40 CUV trip threshold or CUV recovery threshold.
_Avoid_: lockout voltage, CUV

**CUVC**:
A BQ40 safety condition for cell undervoltage computed with cell-resistance compensation. It is not the same condition as raw cell undervoltage.
_Avoid_: cell UV, undervoltage alarm

**CUV**:
A BQ40 safety condition for raw cell undervoltage below the configured threshold.
_Avoid_: cell low, undervoltage

**XDSG**:
A BQ40 logical state indicating that the discharge FET path is disabled.
_Avoid_: discharge block, DSG off
