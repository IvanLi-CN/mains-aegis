# BQ40 charge-temperature profile

> This file is the durable contract for the EVE INR21700/50E 4S1P charge-temperature profile. Runtime implementation facts belong in `IMPLEMENTATION.md`; topic background belongs in `HISTORY.md`.

## Context and Scope

- Context: The project uses `BQ40Z50-R2` as the battery monitor/protection controller and `BQ25792` as the charger. Both devices must apply one conservative, reviewable temperature policy to an EVE INR21700/50E 4S1P pack.
- In scope: cell-temperature boundaries, BQ40 Data Flash temperature fields, BQ40 temperature-dependent charge-current/voltage profiles, BQ25792 JEITA coordination, and the safe read/write boundary for the explicit maintenance writer.
- Out of scope: a new ChemID characterization, pack learning, protection-threshold retuning, or any physical-device operation. The implementation mapping is recorded in `IMPLEMENTATION.md` and is part of this contract's acceptance surface.

## Terms and Interfaces

- Cell temperature: the maximum of BQ40 `TS2`/`TS3`/`TS4`, which are the inter-cell sensors. `TS1` remains the board-side BQ40 sensor and cannot relax the cell-temperature limit.
- Allowed charge window: `0°C <= cell_temperature < 45°C`.
- Low-temperature band: `0°C <= cell_temperature < 15°C`.
- Normal-temperature band: `15°C <= cell_temperature < 45°C`.
- BQ40 temperature fields: `T1`, `T2`, `T5`, `T6`, `T3`, `T4`, and `Hysteresis` at Data Flash addresses `0x4A0B` through `0x4A17`.
- BQ40 DF transport: `ManufacturerBlockAccess` at physical command `0x44`; Data Flash payloads are little-endian and a readback returns the addressed 32-byte block.
- BQ40 charger request: SBS `ChargingCurrent()` `0x14` in mA and `ChargingVoltage()` `0x15` in mV.
- BQ25792 charger target: the requested `ICHG` and `VREG` written by the main firmware after all BMS and charger safety gates pass.

## Requirements

### REQ-BQ40-CTP-001

- The system MUST treat cell temperature at or above `45°C` as no-charge, and MUST NOT support the previously discussed `45–55°C` charge band.
- The system MUST use `max(TS2, TS3, TS4)` for the cell-temperature charge decision; `TS1` MUST NOT extend the allowed cell-temperature window.
- covers: `VER-BQ40-CTP-001`

### REQ-BQ40-CTP-002

- The BQ40 temperature boundaries MUST be `T1=0°C`, `T2=15°C`, `T5=20°C`, `T6=35°C`, `T3=40°C`, `T4=45°C`, with `Hysteresis=2°C`.
- The stored temperature value MUST use the TRM `0.1 K` representation with `round((°C + 273.15) * 10)` and little-endian encoding. The resulting preferred raw values are `2732`, `2882`, `2932`, `3082`, `3132`, `3182`, and `20` respectively.
- The boundaries MUST satisfy `T1 <= T2 <= T5 <= T6 <= T3 <= T4`.
- The seven golden vectors MUST be `AC 0A`, `42 0B`, `74 0B`, `0A 0C`, `3C 0C`, `6E 0C`, and `14 00` in the field order above.
- covers: `VER-BQ40-CTP-002`

### REQ-BQ40-CTP-003

- The low-temperature BQ40 profile MUST request `100mA` and `16,800mV`.
- The STL, RT, STH, and HT BQ40 profiles MUST each request `500mA` and `16,800mV`; this keeps the whole `15–45°C` band on the same normal policy even though `T3=40°C` creates a BQ40 HT classification.
- The BQ40 Low/Med/High current entries within each temperature profile MUST use the same value. Temperature profile changes MUST NOT modify precharge, maintenance-charge, or termination Data Flash fields.
- covers: `VER-BQ40-CTP-003`

### REQ-BQ40-CTP-004

- The main firmware MUST treat the BQ40 `ChargingCurrent()` and `ChargingVoltage()` values as temperature-policy upper bounds and MUST refresh them at least once per second when charging.
- The BQ25792 target MUST be no greater than both the runtime policy target and the BQ40 request. A read failure, zero, invalid, or stale BQ40 request MUST produce an explicit no-charge state.
- In the `100mA` mode, BQ25792 `ITERM` MUST be `40mA` and `EN_TERM` MUST remain disabled according to the existing low-current recovery behavior. Normal-mode termination policy MUST remain the existing BQ40 `CurrentAtEOC`-aligned path and is not part of the T1–T6 profile; an aligned `CurrentAtEOC` value MUST be strictly below the final runtime `ICHG`, otherwise normal charging MUST fail closed. Activation/boot-diagnostic wake charging MUST keep `EN_TERM` disabled because its `200mA` target is a maintenance wake state, not product taper.
- BQ25792 low-temperature JEITA MUST not apply a second current multiplier; `JEITA_ISETC` is `100%` because BQ40 already requests `100mA`. `TS_COOL=15°C`, `TS_WARM=45°C`, `TS_IGNORE=0`, and high-temperature JEITA behavior MUST suspend charging.
- Activation/recovery wake charging is a separate maintenance state, not a product-charge override. Its fixed wake target MUST require the BQ25792 JEITA normal `15–45°C` window. A manually requested offline `Activation`, with no known BQ40 address, MAY enable the bounded wake profile while `VBAT_PRESENT_STAT=0`, but only during its charge-enabled phase and before its absolute deadline. A BMS-isolated PACK terminal can read low even when the cells are installed; `VBAT_PRESENT_STAT=0` MUST NOT by itself block this wake or imply missing cells. Ordinary charging, online discharge-authorization recovery and automatic boot-diagnostic charging MUST retain their existing battery-path gate. The wake limits are `VREG<=16800mV`, `ICHG<=200mA`, `IINDPM=500mA`, with `EN_TERM=0`; the BQ25792 still selects its own low-voltage trickle/precharge stages, so `ICHG=200mA` does not promise 200mA measured battery current. If any BQ40 request value has been observed, including zero or invalid values, or any temperature observation exists, the wake state MUST additionally require a fresh valid request and fresh normal-band BQ40 temperature evidence; a read failure after an observed request MUST remain a no-charge condition. When a request exists, the wake current and voltage MUST be clamped to it. After activation, restoring the saved charger registers MUST leave `CE` high until an ordinary poll re-evaluates the full product-charge gate. A boot-time BMS-missing decision MUST inhibit that moment’s charging, not permanently disable an otherwise reachable charger module; after BMS recovery, ordinary polling MUST be able to evaluate the complete product gate again. Firmware safe mode and an unavailable charger module MUST remain disabled.
- covers: `VER-BQ40-CTP-004`

### REQ-BQ40-CTP-005

- The hardware temperature model MUST use `10kΩ@25°C`, `β=3380` for BQ40 `TS1`–`TS4` and for the separate BQ25792 charger NTC.
- The BQ40 mapping MUST keep `TS1` as board/cell-profile input and `TS2`–`TS4` as cell-temperature inputs. The BQ25792 NTC MUST remain electrically separate, using `RTS1=5.23kΩ` and `RTS2=30.1kΩ`.
- The selected BQ40 project configuration is `Temperature Enable=0x1E` and `Temperature Mode=0x00`; the charge-temperature writer MUST NOT modify these mapping fields.
- covers: `VER-BQ40-CTP-005`

### REQ-BQ40-CTP-006

- The Data Flash writer MUST use an explicit semantic profile and an allowlist containing only `T1`, `T2`, `T5`, `T6`, `T3`, `T4`, and `Hysteresis` for this topic.
- The charge-temperature writer MUST use canonical addressing and the existing strict BQ40 identity probe with both `ManufacturerName()` containing `Texas` and `DeviceName()` containing `BQ40` evidence. The explicit profile MUST be limited to `apply-df` and `verify` actions; diagnostic/recovery actions, `mac-only` and `dual-diag` modes, non-R2 ROM images, and recovery/repair feature combinations MUST be rejected before a profile image can write or verify Data Flash.
- Before writing, it MUST save the complete containing Data Flash block. After writing, it MUST verify the address echo, block length, every target raw value, and every decoded semantic value.
- The writer MUST treat the operation as non-atomic and MUST stop on transport error, write failure, or readback mismatch. A profile failure in the maintenance firmware MUST latch `CE` high and terminate the maintenance charge flow. It MUST NOT silently fall back to the existing broad 25-field `bq40-comm-tool` baseline.
- The audit stream MUST prove the ordered pre-write gates `CE high/charger disabled`, both identity evidence, and Full Access before accepting snapshot, field, and completion evidence.
- The normal `mains-aegis` runtime MUST NOT automatically write BQ40 Data Flash.
- The current/voltage table in the profile manifest is a static policy and validation record only; it MUST NOT become a live Data Flash writer target.
- covers: `VER-BQ40-CTP-006`

### REQ-BQ40-CTP-007

- The pack design inputs MUST be recorded as `4S1P`, `Design Capacity=5000mAh`, `Design Voltage=14600mV`, and `VREG=16800mV`.
- `ChemID` MUST be selected through a TI chemistry-matching procedure and MUST NOT be invented from the cell name or nominal voltage. It is a separate gauging prerequisite, not a temperature-profile write.
- Existing CUV/protection and termination golden settings MUST remain unchanged when applying this temperature profile.
- covers: `VER-BQ40-CTP-007`

## Verification

### VER-BQ40-CTP-001

- Method: Inspect the profile contract and a typed policy fixture using cell temperatures below, inside, and at the upper boundary.
- covers: `REQ-BQ40-CTP-001`
- Pass condition: Values below `0°C` or at/above `45°C` resolve to no-charge, and no fixture permits `45–55°C` charging.

### VER-BQ40-CTP-002

- Method: Run golden-vector encoding tests for all seven Data Flash values and a monotonic-order invariant.
- covers: `REQ-BQ40-CTP-002`
- Pass condition: The vectors produce `AC 0A`, `42 0B`, `74 0B`, `0A 0C`, `3C 0C`, `6E 0C`, and `14 00` in field order.

### VER-BQ40-CTP-003

- Method: Inspect the BQ40 profile manifest and calculate each BQ40 current/voltage field from the semantic profile.
- covers: `REQ-BQ40-CTP-003`
- Pass condition: LT is `100mA/16800mV`; STL/RT/STH/HT are `500mA/16800mV`; no termination or precharge address is in the manifest.

### VER-BQ40-CTP-004

- Method: Host tests for the pure runtime policy seams cover mocked BQ40 `0x14/0x15` request values, freshness, temperature gating, activation wake gating with and without the bounded low-VBAT exception, independent input/temperature/unsafe-source/BQ40-evidence rejection, termination policy, and BQ25792 register encoders; the target build compiles the production orchestration path.
- covers: `REQ-BQ40-CTP-004`
- Pass condition: Runtime charge targets never exceed the BQ40 request, invalid/zero requests block charging, and `100mA` mode uses `ITERM=40mA` with termination disabled. An explicitly bounded offline wake can enable with raw VBAT absent while the ordinary gate still refuses the same absent path; temperature, input, unsafe source and observed-invalid/stale BQ40 evidence still block. Hardware observation MUST distinguish a programmed ICHG limit, enabled CE/EN_CHG, actual charging current, BMS communication recovery and output readiness; reaching the deadline or completing recovery MUST disable the wake before ordinary charging is reconsidered.

### VER-BQ40-CTP-005

- Method: Review the schematic-selection records and the BQ25792/BQ40 temperature mapping configuration.
- covers: `REQ-BQ40-CTP-005`
- Pass condition: BQ40 and BQ25792 use separate NTC circuits with the recorded `10kΩ/β3380` model and resistor values.

### VER-BQ40-CTP-006

- Method: Host MB44 protocol/contract tests cover the exact read and field-write frames, echoed-address readback, malformed length, mismatch, and interrupted-write cases; the target adapter uses those tested frame builders, the profile build requires the strict canonical identity probe, the writer emits strict/R2/canonical provenance plus each actual per-write echo address/length/raw/decoded result, the report parser requires ordered CE/provenance/identity/Full Access gates plus complete pre/post blocks and all seven field-verification records, and ROM/recovery evidence conflicts with a profile pass.
- covers: `REQ-BQ40-CTP-006`
- Pass condition: Only the seven allowlisted fields can change; the report parser compares every non-allowlisted byte between the complete pre-write and post-write blocks and rejects unknown stages, reordered, duplicated, or incomplete evidence whenever the explicit profile is requested; every failure stops the transaction and preserves the pre-write block for rollback; incomplete or inconsistent report evidence is not accepted as PASS.

### VER-BQ40-CTP-007

- Method: Static configuration review against the hardware-selection and BMS design documents.
- covers: `REQ-BQ40-CTP-007`
- Pass condition: Design capacity/voltage and VREG agree with the selected 4S1P pack, while ChemID and unrelated protection fields are not silently changed.

## Related ADRs

- [`../../adr/0002-bq40-charge-temperature-profile.md`](../../adr/0002-bq40-charge-temperature-profile.md)

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
- `../../adr/0002-bq40-charge-temperature-profile.md`
- `../../../tools/bq40-comm-tool/docs/README.md`
- `../../bms-design.md`
- `../../charger-design.md`
- `../../hardware-selection.md`
- `../../manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md`
- `https://www.evemall.eu/consumer-battery/cylindrical-ncm-cell/21700-50e`
