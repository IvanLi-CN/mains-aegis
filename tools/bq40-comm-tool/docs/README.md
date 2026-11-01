# bq40-comm-tool docs

- `operations.md`: end-to-end runbook (diagnose/recover/verify)
- `operations.md`: includes the maintenance-only `bq40-charge-temperature` Data Flash profile
- `log-signatures.md`: log keyword to meaning mapping
- `recovery-safety.md`: ROM recovery safety policy
- `troubleshooting-notes.md`: troubleshooting history and convergence strategy
- `references.md`: external references + recovery image traceability index
- `../firmware/assets/bq40z50_r2_v2_11_build_52/README.md`: active ROM recovery image hash manifest
- `../firmware/assets/bq40z50_r5_v5_05_build_96/README.md`: previous R5 ROM recovery image hash manifest

## Charge-temperature profile

`bq40-charge-temperature` is the only supported BQ40 Data Flash write profile for the
charge-temperature policy. It writes exactly seven temperature fields from the locked
profile and does not write ChemID, calibration, protection, termination, precharge, or
maintenance fields.

Before any field write, the tool saves the complete containing block at `0x4A00` with
length `32`. Each MB44 write must pass address, length, raw-vector, and decoded-value
readback checks; the first error stops the operation. The normal firmware never performs
this write automatically.

The former `live-df-mainboard` profile is retired and returns a migration error. ROM,
AFE, and `asset-df-mainboard` recovery paths retain their existing semantics and are not
aliases for this profile.
