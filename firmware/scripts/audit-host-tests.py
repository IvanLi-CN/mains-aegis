#!/usr/bin/env python3
from pathlib import Path
import sys

REPO = Path(__file__).resolve().parents[2]
SRC = REPO / "firmware" / "src"
ALLOWED = {
    "audio.rs",
    "bq25792.rs",
    "bq40z50.rs",
    "display_pipeline.rs",
    "fan.rs",
    "front_panel_logic.rs",
    "front_panel_scene.rs",
    "mdns_wire.rs",
    "net_bridge.rs",
    "net_contract.rs",
    "net_logic.rs",
    "net_types.rs",
    "output/pure.rs",
    "output_protection.rs",
    "output_retry.rs",
    "output_state.rs",
    "runtime_audio_recovery.rs",
    "tmp112.rs",
    "usb_pd/contract_tracker.rs",
    "usb_pd/mod.rs",
    "usb_pd/pd.rs",
    "usb_pd/sink_policy.rs",
}

failures = []
for path in sorted(SRC.rglob('*.rs')):
    rel = path.relative_to(SRC).as_posix()
    if '#[test]' not in path.read_text():
        continue
    if rel not in ALLOWED:
        failures.append(rel)

if failures:
    print('Host-test audit failed. These firmware/src files still contain #[test] but are not allowlisted for host coverage:')
    for rel in failures:
        print(f'  - {rel}')
    sys.exit(1)

print('Host-test audit passed.')
