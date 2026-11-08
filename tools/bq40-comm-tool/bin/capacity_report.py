"""Strict, single-session audit for the two-block BQ40 capacity writer."""
from __future__ import annotations

import json
import re
from pathlib import Path

PROFILE = "bq40_capacity_model"
BLOCKS = (0x48E0, 0x4840)
FIELDS = (("design_capacity_mah", 0x48E5, bytes((0x88, 0x13)), 5000),
          ("term_voltage_mv", 0x484A, bytes((0xC0, 0x2B)), 11200))


def run(args, common) -> int:
    if args.action not in {"apply-df", "verify"} or args.mode != "canonical":
        print("bq40-capacity-model requires apply-df or verify in canonical mode")
        return 24
    if args.action == "apply-df" and (args.probe_mode != "strict" or args.rom_image != "r2" or args.force_min_charge != "false"):
        print("bq40-capacity-model requires strict/r2 provenance and force-min-charge=false")
        return 24
    if args.action == "verify" and any(v is not None for v in (args.probe_mode, args.rom_image, args.force_min_charge)):
        print("verify does not accept live provenance flags")
        return 24
    source = Path(args.monitor_file).expanduser().resolve()
    if not source.is_file():
        print(f"monitor file not found: {source}")
        return 12
    # Exact order, shapes and counts prevent assembling a pass from duplicated,
    # partial, interleaved or reordered evidence. Both snapshots precede fields.
    expected = [
        ("charge_disabled", "ce_high=true charger_enabled=false"),
        ("provenance", "probe_mode=strict rom_image=r2 mode=canonical"),
        ("identity_verified", "manufacturer=texas device=bq40"),
        *[(s, "write=ok") for s in common.CHARGE_PROFILE_SECURITY_STAGE_ORDER],
        ("security_after", r"op=0x[0-9a-fA-F]+ sec=full_access"),
        ("firmware_verified", "device_type=0x4500 fw_major=2"),
        ("prerequisites_verified", "cell_count=4 cell_term=false"),
        ("block_before", r"df_addr=0x48[eE]0 len=32"), ("snapshot_before_hex", None),
        ("block_before", "df_addr=0x4840 len=32"), ("snapshot_before_hex", None),
        ("field_verify", None), ("field_verify", None),
        ("block_after", r"df_addr=0x48[eE]0 len=32"), ("final_after_hex", None),
        ("block_after", "df_addr=0x4840 len=32"), ("final_after_hex", None),
        ("done", "fields=2 blocks=2"),
        ("halt", "reason=complete charger_enabled=false"),
    ]
    errors = []
    events = []
    seen = {}
    addresses = set()
    snapshots = []
    finals = []
    verified = []
    in_preexisting = False
    allow_preexisting = False
    parse_preexisting = False
    for line in source.read_text().splitlines():
        if not line.strip():
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            errors.append("malformed_json")
            continue
        if not isinstance(entry, dict):
            errors.append("malformed_entry")
            continue
        if entry.get("src") == "meta":
            event = entry.get("event")
            if event == common.SESSION_BOUNDARY_EVENT:
                if events:
                    errors.append("session_boundary")
                allow_preexisting = False
            elif event == common.RECENT_EXISTING_STDOUT_EVENT:
                allow_preexisting = True
            elif event == common.PREEXISTING_BEGIN_EVENT:
                in_preexisting = True
                parse_preexisting = allow_preexisting
                allow_preexisting = False
            elif event == common.PREEXISTING_END_EVENT:
                in_preexisting = False
                parse_preexisting = False
            continue
        if in_preexisting and not parse_preexisting:
            continue
        text = entry.get("text", "")
        if not isinstance(text, str):
            errors.append("malformed_text")
            continue
        if common.ROM_OR_RECOVERY_EVENT_RE.search(text):
            errors.append("recovery_conflict")
        if events and events[-1][0] == "halt" and (common.CHARGE_PROFILE_POST_HALT_CHARGE_ENABLE_RE.search(text) or re.search(r"\bcharger:.*\benabled=true\b", text)):
            errors.append("post_halt_charge_enable")
        if "bms_df_apply:" not in text:
            continue
        key = common.dedupe_key(entry, text)
        if key is not None:
            src = entry.get("src")
            if not isinstance(src, str):
                errors.append("malformed_source")
                continue
            sources = seen.setdefault(key, set())
            if common.is_mirrored_profile_evidence(sources, src):
                sources.add(src)
                continue
            sources.add(src)
        m = re.fullmatch(r"(?:\[[A-Z ]+\]\s+)?bms_df_apply: addr=0x([0-9a-fA-F]+) profile=(\S+) stage=(\S+)(?: (.*))?", text)
        if not m or m[2] != PROFILE:
            errors.append("unknown_or_malformed_profile")
            continue
        address = int(m[1], 16)
        addresses.add(address)
        stage, tail = m[3], m[4] or ""
        index = len(events)
        events.append((stage, tail))
        if index >= len(expected) or stage != expected[index][0]:
            errors.append("evidence_sequence_invalid")
            continue
        pattern = expected[index][1]
        if pattern is not None and not re.fullmatch(pattern, tail):
            errors.append(f"invalid_{stage}")
        if stage == "security_after":
            op = re.search(r"op=0x([0-9a-fA-F]+)", tail)
            if op is None or int(op[1], 16) & 0x300 != 0x100:
                errors.append("security_bits_not_full_access")
        if stage in {"snapshot_before_hex", "final_after_hex"}:
            tokens = tail.split()
            if len(tokens) != 32 or any(not re.fullmatch(rf"b{i}=0x[0-9a-fA-F]{{1,2}}", t) for i, t in enumerate(tokens)):
                errors.append("invalid_block_payload")
                continue
            payload = bytes(int(t.split("=", 1)[1], 16) for t in tokens)
            (snapshots if stage == "snapshot_before_hex" else finals).append(payload)
        if stage == "field_verify":
            i = len(verified)
            if i >= len(FIELDS):
                errors.append("extra_field")
                continue
            name, df_addr, raw, decoded = FIELDS[i]
            fm = re.fullmatch(r"field=(\w+) df_addr=0x([0-9a-fA-F]+) declared_len=(\d+) payload_len=(\d+) raw_lo=0x([0-9a-fA-F]{1,2}) raw_hi=0x([0-9a-fA-F]{1,2}) decoded=0x([0-9a-fA-F]{1,4})", tail)
            if not fm or (fm[1], int(fm[2], 16), int(fm[3]), int(fm[4]), int(fm[5], 16), int(fm[6], 16), int(fm[7], 16)) != (name, df_addr, 32, 32, *raw, decoded):
                errors.append("field_verification_invalid")
            verified.append(name)
    if len(events) != len(expected):
        errors.append("incomplete_evidence")
    if addresses != {0x0B}:
        errors.append("device_address_invalid")
    if len(snapshots) != 2 or len(finals) != 2:
        errors.append("block_evidence_missing")
    else:
        for i, block in enumerate(BLOCKS):
            target = bytearray(snapshots[i])
            _, address, raw, _ = FIELDS[i]
            offset = address - block
            target[offset:offset + 2] = raw
            if target != finals[i]:
                errors.append(f"block_0x{block:04x}_mismatch")
    errors = sorted(set(errors))
    summary = {
        "profile": "bq40-capacity-model", "source_log": str(source),
        "run_config": {k: getattr(args, k) for k in ("action", "mode", "repair_profile", "force_min_charge", "probe_mode", "rom_image")},
        "blocks": [{"address": f"0x{address:04x}", "snapshot_before_hex": snapshots[i].hex() if i < len(snapshots) else None,
                    "final_after_hex": finals[i].hex() if i < len(finals) else None} for i, address in enumerate(BLOCKS)],
        "verified_fields": verified, "verified_field_count": len(verified),
        "writes": 2 if not errors else None,
        "verdict": {"pass": not errors, "reason": ";".join(errors) or "ok"},
    }
    out = Path(args.report_out).expanduser().resolve()
    out.mkdir(parents=True, exist_ok=True)
    (out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    (out / "summary.md").write_text("# BQ40 容量配置应用结果\n\n" + f"- profile: `bq40-capacity-model`\n- pass: `{not errors}`\n- reason: `{summary['verdict']['reason']}`\n\n" + "```json\n" + json.dumps(summary, ensure_ascii=False, indent=2) + "\n```\n")
    print(out / "summary.json")
    print(out / "summary.md")
    return 0 if not errors else 20
