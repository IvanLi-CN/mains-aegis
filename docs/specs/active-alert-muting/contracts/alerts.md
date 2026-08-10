# Active Alert Muting Contract

> This contract is implemented only after the front-panel preview approval gate in [`../SPEC.md`](../SPEC.md). Until then, no transport exposes a write path.

## Common model

```json
{
  "alert_id": "mains_absent_dc",
  "instance_id": "opaque-current-instance-id",
  "severity": "warning",
  "sound_state": "audible",
  "summary": "RUNNING ON BATTERY"
}
```

- `alert_id` is one of the nine IDs listed by the topic spec.
- `instance_id` is opaque. A client must return it unchanged when muting.
- `severity` is `warning` or `critical`.
- `sound_state` is `audible`, `muted`, `system_silent`, or `policy_silent`.

## Device transports

USB CDC defines two commands:

```text
get_alerts
mute_alert { alert_id, instance_id }
```

LAN HTTP defines their direct equivalents:

```text
GET  /api/v1/alerts
POST /api/v1/alerts/{alert_id}/mute
```

The HTTP request body is:

```json
{ "instance_id": "opaque-current-instance-id" }
```

`GET` returns `200` with `{"alerts":[...]}`. A successful `POST` returns `200` with the authoritative current alert item and `{"result":"muted"}`. The device returns structured failure payloads:

| Result | HTTP status | Meaning |
| --- | --- | --- |
| `stale` | `409` | The ID exists, but the supplied instance is not current. |
| `inactive` | `409` | The target is no longer active. |
| `unsupported` | `501` | Firmware predates this contract. |

CDC uses the same `result` values in its response envelope. In every success and failure response the device remains the authority; a client refreshes `get_alerts` before completing a user action.

## devd and CLI

devd exposes IPC methods corresponding one-to-one with `get_alerts` and `mute_alert`, and forwards the same response envelope through its device HTTP bridge. It must not infer muting support from telemetry.

The CLI is fixed to:

```text
mains-aegis device <id> alerts list
mains-aegis device <id> alerts mute <alert-id>
```

Both commands emit machine-readable JSON. `alerts mute` first reads the authority record for `<alert-id>`, sends its current `instance_id`, then prints the authoritative post-action response. A concurrent resolution or recurrence therefore produces `inactive` or `stale`, never a mute of the new instance.

## Web client behavior

- The `Alerts` page reads the authoritative collection and exposes one mute icon per active alert.
- A row is locked while its request is in flight; after success the client re-reads the collection before rendering the settled state.
- `offline`, `unsupported`, `stale`, `inactive`, and transport failure are explicit UI states. The client never presents a mute affordance based only on telemetry.
