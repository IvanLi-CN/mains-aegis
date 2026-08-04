# Mains Aegis Device Operation Guardrails

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## Background

Mains Aegis development can involve multiple devices and serial candidates. Agent-driven diagnostics or maintenance must avoid accidental operations on an unintended device.

## Goals

- Keep Mains Aegis hardware access on the owner-visible `mains-aegis` / `mains-aegis-devd` route.
- Prevent direct flasher use, legacy device-control paths, direct serial-port enumeration, and speculative port switching.
- Require explicit owner authorization for persistent bindings, settings writes, reset, flash, and real host power actions.

## Non-goals

- This specification does not permit automatic recovery, port switching, or trial-and-error device selection.
- This specification does not change the recovery tool's own CLI output.

## Guardrails

### G0: No Direct Flasher

Agents must not invoke `espflash`, `cargo espflash`, or `cargo-espflash` directly. The devd backend may use its own internal flash backend only through the approved flow.

### G1: No Legacy Device Path

Agents must not use `mcu-agentd` for Mains Aegis device operations.

### G2: No Direct Port Enumeration

Agents must not inspect `/dev/*` or other serial-device paths to discover candidate ports.

### G3 and G4: No Port Switching

Agents must not switch ports or try another port based on discovery results, history, or failure recovery.

### G5: Approved Device Route

In-repository development uses `$mains-aegis-devd-flow` and the `mains-aegis` CLI lifecycle surface. Owner-visible devd scan/list, connection lifecycle operations, and read-only status checks are allowed within that flow. Persistent binding changes, settings writes, reset, flash, and real host power actions require an explicit bound-device context and owner authorization.

## Acceptance

- Repository guidance retains G0 through G5 without allowing prohibited device or port operations.
- Hardware collaboration documentation and Agent instructions remain consistent with this specification.
- The legacy device-operation-guardrails planning directory is retired and all active references resolve to this canonical specification.

## References

- [Repository guidelines](../../../AGENTS.md)
- [Hardware collaboration workflow](../../hardware-collaboration-workflow.md)

## Visual Evidence

PR: none
