# Runtime UPS Output Bypass

## Purpose

Bench tests sometimes need to measure the direct input/pass-through path without
the UPS output stage contributing battery current. This is a temporary runtime
control, not an `advanced_power` setting.

## Interface

Through the normal Mains Aegis devd and USB CDC path:

- `enable_output_bypass`: accepted only while VIN is stable; saves the current
  output request, disables both TPS output stages, and reports gate reason
  `manual_bypass`.
- `restore_output`: clears the RAM-only override and re-admits the saved output
  request through the normal safety gates.

The CLI entry point is:

```bash
mains-aegis device <saved-device-id> output-bypass --enable
mains-aegis device <saved-device-id> output-bypass --restore
```

The override is cleared on reboot. It does not disable VIN sensing, BMS,
thermal protection, charger input sensing, or TPS fault handling. It only
removes the UPS output stage from the experiment. Enabling it while VIN is
absent is rejected because that would silently remove the backup source.

## Verification

After enabling, use `device status` or `device diag-snapshot` and verify that
requested and active outputs are `none`, with gate reason `manual_bypass`.
Always restore the output before returning the hardware to normal UPS use.

This is a diagnostic/bench control, not a production UPS mode and not a
replacement for source-limited backup takeover.

Normal main firmware otherwise preserves its configured `both` request even
when only one channel is healthy. A single-channel or `none` request is valid
only through an explicit diagnostic/test profile or control such as this one;
the boot self-check must never infer that downgrade from the healthy subset.
