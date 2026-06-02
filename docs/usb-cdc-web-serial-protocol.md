# USB CDC / Web Serial Protocol

## Scope

This protocol is the first Web App write path for Mains Aegis. It runs over ESP32-S3 USB Serial/JTAG CDC and is consumed directly by Chromium Web Serial.

LAN HTTP/SSE remains read-only. The USB CDC protocol only accepts safe settings:

- WiFi SSID/PSK overwrite or clear.
- Manual charge preferences.
- USB session log level.
- Identity and status reads.

High-risk operations such as output enable/disable, fault clear, and charge start/stop are not part of this protocol.

## Mains Aegis Device Daemon

`mains-aegis-devd` is the preferred local device owner. It manages device scan/list/bind/connect/disconnect/unbind, firmware artifact selection, flash/reset/monitor operations, host power coordination, and USB CDC safe-control. Its default `serve` mode is IPC-only; HTTP/Web access is exposed only through explicit `bridge-http`.

Development mode uses the Web dev server as the API reverse proxy: the browser talks to the Vite origin, and Vite proxies `/api` to the explicit `bridge-http` endpoint, default `http://127.0.0.1:30080`. Production mode may serve static Web assets directly from `bridge-http`.

Device management endpoints:

- `GET /api/v1/devices`
- `POST /api/v1/devices/scan`
- `POST /api/v1/devices/{id}/bind`
- `POST /api/v1/devices/{id}/connect`
- `POST /api/v1/devices/{id}/disconnect`
- `DELETE /api/v1/devices/{id}/binding`
- `GET /api/v1/devices/{id}/identity`
- `GET|POST /api/v1/devices/{id}/artifact`
- `POST /api/v1/devices/{id}/flash`
- `POST /api/v1/devices/{id}/reset`
- `POST /api/v1/devices/{id}/monitor/start`
- `POST /api/v1/devices/{id}/monitor/stop`
- `GET /api/v1/devices/{id}/session`
- `GET /api/v1/devices/{id}/events`

devd local safe-control endpoints:

- `GET /health`
- `GET /api/v1/ping`
- `GET /api/v1/identity`
- `GET /api/v1/network`
- `GET /api/v1/status`
- `GET /api/v1/serial/session?logs_limit=<n>&trace_limit=<n>`
- `POST /api/v1/wifi-config`
- `DELETE /api/v1/wifi-config`
- `POST /api/v1/settings/log-level`
- `POST /api/v1/settings/manual-charge`
- `GET /api/v1/host/power`
- `POST /api/v1/host/power/profile`
- `POST /api/v1/host/power/suspend`
- `POST /api/v1/host/power/shutdown`
- `GET /api/v1/host/power/events`

`/api/v1/serial/session` returns Web-compatible `logs`, `trace`, `protocol`, and `safeSettings`. devd keeps bounded in-memory ring buffers and returns a bounded tail by default (`logs_limit=200`, `trace_limit=600`; capped at `500` and `2000`). TX trace payloads redact WiFi PSK before storage or HTTP exposure.

Safe-control writes are device-scoped. Web/App/CLI callers pass firmware `identity.device_id` as `device_id` in POST bodies, or as the `DELETE /api/v1/wifi-config?device_id=...` query parameter. If `device_id` is omitted, devd only accepts the request when exactly one native USB CDC device is connected with identity available.

Host power control is host-scoped, not device-scoped. It exists for UPS-on-battery coordination when the computer should reduce load while continuing to run. `low_power_running` means a platform power profile such as Linux `power-saver` or macOS Low Power Mode; it is not suspend/sleep. All host power state-changing requests default to `dry_run=true`. Real profile switching, suspend, and shutdown are denied unless devd was started with `--allow-host-power-actions` or `MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS=true`; real shutdown also requires `confirm:"shutdown"`. devd executes the Web App or UPS instruction as requested and does not add shutdown policy decisions; callers must pass `force:true` explicitly when the UPS power-loss window requires forced system compliance. On Linux, forced shutdown is only supported for `delay_sec:0`; nonzero forced delays return `host_power_shutdown_unsupported` because systemd cannot reliably express both at once. On macOS, `force:true` also returns `host_power_shutdown_unsupported` because the pmset/shutdown backend has no forced-compliance command.

Profile request example:

```json
{"profile":"power_saver","dry_run":true}
```

Shutdown request example:

```json
{"delay_sec":60,"dry_run":true,"force":false}
```

Dry-run responses include `backend`, `action`, target profile or delay, `force`, `scheduled_after_sec` for shutdown, and the command that would be executed. Real shutdown returns `dispatch:"command_accepted"` only after the platform command exits successfully; devd does not perform an internal shutdown countdown or make an extra policy decision. `GET /api/v1/host/power/events` streams `host_power` SSE events so local listeners can react to UPS load-shed and graceful-shutdown decisions.

## Framing

- Encoding: UTF-8 JSON.
- Framing: one JSON object per line, terminated by LF (`\n`).
- Protocol name: `mains-aegis.cdc.v1`.
- Every Web write command carries `request_id`.
- Firmware returns `response` or `error` with the same `request_id`.
- During bring-up, legacy `defmt` or plain serial bytes may still appear on the same CDC stream. Browser clients ignore non-JSONL and malformed non-protocol lines; protocol responses must still be valid JSONL frames.

## Frame Types

### `hello`

Web:

```json
{"type":"hello","request_id":"web-1"}
```

Firmware:

```json
{"type":"hello","request_id":"web-1","protocol":"mains-aegis.cdc.v1","framing":"jsonl","capabilities":{"status":true,"structured_logs":true,"safe_settings":true,"wifi_config":true,"psk_echo":false},"identity":{}}
```

`identity` uses the same shape as `/api/v1/identity`; USB identity reports `capabilities.write_controls=true`.

### `request`

Supported `op` values:

- `get_identity`
- `get_status`
- `set_log_level`
- `set_manual_charge_prefs`

Examples:

```json
{"type":"request","request_id":"web-2","op":"get_status"}
{"type":"request","request_id":"web-3","op":"set_log_level","level":"debug"}
{"type":"request","request_id":"web-4","op":"set_manual_charge_prefs","target":"rsoc_80","speed":"ma_500","timer_h":2}
```

### `wifi_config`

Set:

```json
{"type":"wifi_config","request_id":"web-5","op":"set","ssid":"LabNet","psk":"example-password"}
```

Clear:

```json
{"type":"wifi_config","request_id":"web-6","op":"clear"}
```

The firmware stores the PSK but never echoes it in `response`, `error`, or `log` frames.

### `response`

```json
{"type":"response","request_id":"web-5","ok":true,"result":{"wifi_configured":true,"psk_present":true,"psk_echoed":false,"ssid":"LabNet"}}
```

### `status`

```json
{"type":"status","status":{}}
```

`status` uses the same shape as `/api/v1/status`.

### `log`

```json
{"type":"log","level":"info","target":"wifi_config","message":"WiFi credentials updated in EEPROM"}
```

`log` frames are structured Web-facing events. A successful `hello` emits a `usb_cdc` session log. `get_status` emits an initial status log set for `status`, `output`, `charger`, `battery`, and `network`; later status requests emit periodic summaries and state-change logs for those targets.

The Web App records per-session CDC trace entries for developer inspection: transmitted request frames, received protocol frames, structured logs, and raw / ignored non-protocol CDC lines. devd-backed sessions expose a bounded tail to the browser while keeping the local ring buffer bounded in memory. WiFi PSK values are redacted from transmitted trace payloads. Raw firmware monitor output is decoded only when `mains-aegis-devd` has a selected artifact whose identity matches the connected device; otherwise it remains `unverified`.

### `error`

```json
{"type":"error","request_id":"web-5","error":{"code":"invalid_wifi_psk","message":"WiFi PSK must be 8..63 non-control bytes","retryable":false,"details":null}}
```

The `error` envelope matches the HTTP API error shape.

## WiFi Config Storage

The firmware stores WiFi credentials in EEPROM plaintext by current project decision.

- Offset: `0x0160`.
- Size: 128 bytes, written as four 32-byte EEPROM blocks.
- Layout: magic `MAWF`, version, enabled flag, SSID length, PSK length, SSID bytes, PSK bytes, CRC8.
- Empty or clear record decodes as no configured WiFi credentials.
- When `net_http` is enabled, the firmware loads this EEPROM record before starting WiFi and updates the running WiFi task after a successful USB write. Clearing the record wipes the EEPROM slot, disconnects WiFi immediately, and leaves WiFi disabled until new credentials are written over USB.

The UI must clear the PSK field after submit and must not display stored PSK values.
