# MX5050T datasheet (Wuxi Maxin Micro / 无锡明芯微电子有限公司)

This folder stores an offline-renderable mirror of the MX5050T ideal-diode / high-side OR-ing FET controller datasheet.

## Source

- PDF (internal mirror): https://webdav-syncthing.ivanli.cc/Ivan-Personal/Datasheets/Power/%E7%90%86%E6%83%B3%E4%BA%8C%E6%9E%81%E7%AE%A1/MX5050T.pdf

## Offline mirror (this repo)

- Generated Markdown (**do not edit by hand**): `MX5050T.md`
- Extracted images: `images/`
- Original PDF (fidelity): `MX5050T.pdf`

## MOSFET selection notes (from datasheet)

MX5050T drives an external **N-channel MOSFET** as an “ideal diode” / OR-ing element.

- Key MOSFET parameters called out by the datasheet: `ID`, `IS` (body diode), `VDS(max)`, `V(BR)DSS`, `VGS(th)`, `RDS(on)`, `Qg`.
- Gate-drive capability (MX5050T): `IGATE(ON)` is only ~30µA, so turn-on is slow for large `Qg` parts (`tON ≈ Qg / IGATE(ON)`). `IGATE(OFF)` is 2A peak, so turn-off can be fast.
- `RDS(on)` guideline in the datasheet: pick `RDS(on)` such that the forward drop at nominal load current is **30–100mV**:
  - `(30mV / ILOAD) < RDS(on) < (100mV / ILOAD)`

## NCEP3040Q quick evaluation (recommended default)

Project context: UPS OUT max current is `6.32A` (see `docs/ups-output-design.md`).

- For `ILOAD = 6.32A`, the datasheet `RDS(on)` guideline implies `RDS(on) ≈ 4.75–15.8mΩ`.
- NCEP3040Q key parameters (Wuxi NCE Power / datasheet):
  - `VDS = 30V`, `VGS = ±20V`
  - `RDS(on)` (TC=25°C): `≈6.8mΩ @ VGS=10V` (typ), `≈9.5mΩ @ VGS=4.5V` (typ)
  - `Qg ≈ 15nC` (typ)

Conclusion:

- **Aligned with the datasheet’s `RDS(on)` guideline** for OR-ing at `6.32A` (forward drop falls in the intended 30–100mV range).
- **Good match for MX5050T turn-on capability** (`IGATE(ON)` is small), because `Qg` is relatively low.

## NCEP3065QU quick evaluation (low-drop option)

Project context: UPS OUT max current is `6.32A` (see `docs/ups-output-design.md`).

- For `ILOAD = 6.32A`, the datasheet `RDS(on)` guideline implies `RDS(on) ≈ 4.75–15.8mΩ`.
- NCEP3065QU key parameters (Wuxi NCE Power / `http://www.ncepower.com`, datasheet v6.0):
  - `VDS = 30V`, `VGS = ±20V`
  - `RDS(on)` (TC=25°C, ID=20A): `1.6/1.9/2.3mΩ @ VGS=10V` (min/typ/max), `2.5/3.0/3.6mΩ @ VGS=4.5V` (min/typ/max)
  - `Qg = 34.8nC` (typ, VDS=15V, ID=20A, VGS=10V)

Conclusion:

- **Electrically compatible** with MX5050T gate drive (`VGS` rating is fine; `Qg` is moderate).
- **Not aligned with the datasheet’s `RDS(on)` guideline** for OR-ing at `6.32A` (its `RDS(on)` is much lower), which can push reverse-detection to higher reverse currents when supplies are closely matched.
- **Voltage margin**: `30V VDS` may be OK for a 12/19V system, but validate worst-case transients on `VBUS/UPS OUT` (or prefer a higher-`VDS` MOSFET if spikes are possible).
