#!/usr/bin/env python3
"""Offline capacity audit contracts; never connects to a device."""
import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

BIN = Path(__file__).parent
spec = importlib.util.spec_from_file_location("capacity_report", BIN / "capacity_report.py")
capacity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capacity)


def line(stage, tail):
    return json.dumps({"text": f"bms_df_apply: addr=0xb profile=bq40_capacity_model stage={stage} {tail}"})


def successful_log():
    lines = [line("charge_disabled", "ce_high=true charger_enabled=false"),
             line("provenance", "probe_mode=strict rom_image=r2 mode=canonical"),
             line("identity_verified", "manufacturer=texas device=bq40")]
    for s in ("security_unseal_0414", "security_unseal_3672", "security_full_access_ffff_1", "security_full_access_ffff_2"):
        lines.append(line(s, "write=ok"))
    lines.extend([line("security_after", "op=0x100 sec=full_access"),
                  line("firmware_verified", "device_type=0x4500 fw_major=2"),
                  line("prerequisites_verified", "cell_count=4 cell_term=false")])
    for address in capacity.BLOCKS:
        lines += [line("block_before", f"df_addr=0x{address:x} len=32"),
                  line("snapshot_before_hex", " ".join(f"b{i}=0xa5" for i in range(32)))]
    for name, address, raw, value in capacity.FIELDS:
        lines.append(line("field_verify", f"field={name} df_addr=0x{address:x} declared_len=32 payload_len=32 raw_lo=0x{raw[0]:x} raw_hi=0x{raw[1]:x} decoded=0x{value:x}"))
    for block, (_, address, raw, _) in zip(capacity.BLOCKS, capacity.FIELDS):
        payload = bytearray([0xa5] * 32)
        payload[address-block:address-block+2] = raw
        lines += [line("block_after", f"df_addr=0x{block:x} len=32"),
                  line("final_after_hex", " ".join(f"b{i}=0x{v:x}" for i, v in enumerate(payload)))]
    return lines + [line("done", "fields=2 blocks=2"), line("halt", "reason=complete charger_enabled=false")]


class AuditTests(unittest.TestCase):
    def parse(self, lines, *extra):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            log = root / "input.ndjson"
            log.write_text("\n".join(lines) + "\n")
            result = subprocess.run(["python3", str(BIN / "report_parser.py"), "--action", "verify", "--mode", "canonical", "--duration-sec", "120", "--monitor-file", str(log), "--repair-profile", "bq40-capacity-model", "--report-out", str(root / "out"), *extra], capture_output=True, text=True)
            summary = root / "out" / "summary.json"
            return result, json.loads(summary.read_text()) if summary.exists() else None

    def test_complete_evidence_preserves_both_full_blocks(self):
        result, summary = self.parse(successful_log())
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertTrue(summary["verdict"]["pass"])
        self.assertEqual(summary["writes"], 2)
        self.assertEqual(len(summary["blocks"]), 2)
        self.assertEqual(summary["verified_fields"], [f[0] for f in capacity.FIELDS])

    def test_every_required_event_is_required(self):
        good = successful_log()
        for i in range(len(good)):
            with self.subTest(index=i):
                result, summary = self.parse(good[:i] + good[i+1:])
                self.assertEqual(result.returncode, 20)
                self.assertFalse(summary["verdict"]["pass"])

    def test_duplicate_and_reordered_events_are_rejected(self):
        good = successful_log()
        for i in range(len(good)):
            result, _ = self.parse(good[:i] + [good[i]] + good[i:])
            self.assertEqual(result.returncode, 20)
        good[10], good[14] = good[14], good[10]
        self.assertEqual(self.parse(good)[0].returncode, 20)

    def test_corrupt_addresses_lengths_targets_and_collateral_are_rejected(self):
        for old, new in [("df_addr=0x48e0", "df_addr=0x48e1"), ("len=32", "len=31"),
                         ("decoded=0x1388", "decoded=0x1130"), ("raw_lo=0x88", "raw_lo=0x30"),
                         ("fw_major=2", "fw_major=5"), ("sec=full_access", "sec=unsealed"), ("op=0x100", "op=0x300"),
                         ("ce_high=true", "ce_high=false"), ("cell_count=4", "cell_count=3"),
                         ("addr=0xb", "addr=0x16")]:
            with self.subTest(value=old):
                self.assertEqual(self.parse([s.replace(old, new) for s in successful_log()])[0].returncode, 20)
        for block_index in (17, 19):
            good = successful_log()
            good[block_index] = good[block_index].replace("b0=0xa5", "b0=0xa6")
            self.assertEqual(self.parse(good)[0].returncode, 20)

    def test_failure_session_boundary_wrong_profile_and_recovery_are_rejected(self):
        for extra in [line("error", "error=write_transport df_addr=0x484a"),
                      json.dumps({"src": "meta", "event": "monitor_session_start"}),
                      line("done", "fields=2 blocks=2").replace("bq40_capacity_model", "bq40_charge_temperature"),
                      json.dumps({"text": "bms_rom: stage=flash_done"}),
                      "malformed JSON"]:
            self.assertEqual(self.parse(successful_log()[:15] + [extra] + successful_log()[15:])[0].returncode, 20)

    def test_post_halt_enable_is_rejected(self):
        good = successful_log() + [json.dumps({"text": "charger: enabled=true"})]
        self.assertEqual(self.parse(good)[0].returncode, 20)

    def test_mirrored_events_are_deduplicated_but_real_duplicates_are_not(self):
        lines = []
        for i, event in enumerate(successful_log()):
            entry = json.loads(event)
            for src in ("trace", "log"):
                lines.append(json.dumps({**entry, "src": src, "ts": f"2026-11-08T00:00:{i:02d}Z"}))
        self.assertEqual(self.parse(lines)[0].returncode, 0)
        lines.insert(1, lines[0])
        self.assertEqual(self.parse(lines)[0].returncode, 20)

    def test_preexisting_only_evidence_cannot_pass(self):
        meta = lambda event: json.dumps({"src": "meta", "event": event})
        self.assertEqual(self.parse([meta("preexisting_segment_begin"), *successful_log(), meta("preexisting_segment_end")])[0].returncode, 20)

    def test_verify_cannot_supply_live_provenance(self):
        self.assertEqual(self.parse(successful_log(), "--rom-image", "r2")[0].returncode, 24)


if __name__ == "__main__":
    unittest.main()
