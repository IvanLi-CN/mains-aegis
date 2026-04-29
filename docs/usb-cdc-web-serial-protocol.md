# USB CDC / Web Serial Protocol

## Scope

This protocol is the first Web App write path for Mains Aegis. It runs over ESP32-S3 USB Serial/JTAG CDC and is consumed directly by Chromium Web Serial.

LAN HTTP/SSE remains read-only. The USB CDC protocol only accepts safe settings:

- WiFi SSID/PSK overwrite or clear.
- Manual charge preferences.
- USB session log level.
- Identity and status reads.

High-risk operations such as output enable/disable, fault clear, and charge start/stop are not part of this protocol.

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

`log` frames are structured Web-facing events. Raw firmware monitor output remains a development-only stream decoded by `mcu-agentd monitor`; the Web log panel does not display undecoded raw serial bytes.

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
- When `net_http` is enabled, the firmware loads this EEPROM record before starting WiFi and updates the running WiFi task after a successful USB write. Clearing the record falls back to build-time `MAINS_AEGIS_WIFI_*` credentials.

The UI must clear the PSK field after submit and must not display stored PSK values.
