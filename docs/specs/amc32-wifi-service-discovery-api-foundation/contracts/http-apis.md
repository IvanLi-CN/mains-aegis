# HTTP API

统一约束：

- API version: `v1`
- Base path: `/api/v1`
- Auth: `none`
- Content type: `application/json; charset=utf-8`
- JSON naming: `snake_case`
- Error envelope:

```json
{
  "error": {
    "code": "not_found",
    "message": "not found",
    "retryable": false,
    "details": null
  }
}
```

- CORS / compatibility headers:
  - `Access-Control-Allow-Origin: <Origin|*>`
  - `Access-Control-Allow-Methods: GET, OPTIONS`
  - `Access-Control-Allow-Headers: Accept, Content-Type`
  - `Access-Control-Allow-Private-Network: true`
- `OPTIONS` 仅对 `/health` 与 `/api/v1/*` 返回兼容响应。

## Ping / Health（GET `/api/v1/ping` and GET `/health`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

- Headers: None
- Query: None
- Body: None

### 响应（Response）

- Success:

```json
{ "ok": true }
```

- Error: None（仅传输层错误）

### 错误（Errors）

- `404/not_found`: unknown path（retryable: no）

### 示例（Examples）

- Request:

```http
GET /api/v1/ping HTTP/1.1
Host: mains-aegis-a1b2c3.local
```

- Response:

```json
{ "ok": true }
```

### 兼容性与迁移（Compatibility / migration）

- `GET /health` 与 `GET /api/v1/ping` 语义等价，供基础探活和兼容旧式 health-check 使用。

## Identity（GET `/api/v1/identity`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

- Headers: None
- Query: None
- Body: None

### 响应（Response）

- Success schema:

```json
{
  "device_id": "mains-aegis-a1b2c3",
  "hostname": "mains-aegis-a1b2c3",
  "hostname_fqdn": "mains-aegis-a1b2c3.local",
  "short_id": "a1b2c3",
  "role": "ups",
  "api_version": "v1",
  "firmware": {
    "package_version": "0.1.0",
    "build_profile": "dev",
    "build_id": "abc123-clean-deadbeef",
    "git_sha": "abc123",
    "src_hash": "deadbeef",
    "git_dirty": "clean"
  },
  "network": {
    "state": "connected",
    "ipv4": "192.168.31.42",
    "gateway": "192.168.31.1",
    "dns": "1.1.1.1",
    "is_static": false,
    "last_error": null,
    "rssi_dbm": null
  },
  "capabilities": {
    "sse": true,
    "mdns": true,
    "dns_sd": true,
    "write_controls": false
  }
}
```

- Error: standard error envelope

### 错误（Errors）

- `503/unavailable`: identity not ready（retryable: yes）

### 示例（Examples）

- Request:

```http
GET /api/v1/identity HTTP/1.1
Host: mains-aegis-a1b2c3.local
```

### 兼容性与迁移（Compatibility / migration）

- `device_id`、`hostname`、DNS-SD `device_id` TXT 必须保持一致；后续版本只允许新增字段，不改名。

## Network（GET `/api/v1/network`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

- Headers: None
- Query: None
- Body: None

### 响应（Response）

- Success schema:

```json
{
  "device_id": "mains-aegis-a1b2c3",
  "hostname": "mains-aegis-a1b2c3",
  "hostname_fqdn": "mains-aegis-a1b2c3.local",
  "state": "connecting",
  "ipv4": null,
  "gateway": null,
  "dns": "1.1.1.1",
  "is_static": false,
  "last_error": "dhcp_timeout",
  "rssi_dbm": null
}
```

- `state` enum: `disabled | idle | connecting | connected | error`
- `last_error` enum: `bad_static_config | connect_failed | dhcp_timeout | link_lost | null`

### 错误（Errors）

- `503/unavailable`: identity not ready（retryable: yes）

### 兼容性与迁移（Compatibility / migration）

- `rssi_dbm` 首版允许恒为 `null`；后续若补齐真实 RSSI，不需要改版本。

## Status（GET `/api/v1/status`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

- Headers: None
- Query: None
- Body: None

### 响应（Response）

- Success schema（节选）:

```json
{
  "mode": "standby",
  "input": {
    "mains_present": true,
    "input_vbus_mv": 19240,
    "input_ibus_ma": 1180,
    "vin_vbus_mv": 19240,
    "vin_iin_ma": 1180
  },
  "output": {
    "requested": "both",
    "active": "out_a",
    "recoverable": "both",
    "gate_reason": "none",
    "out_a": {
      "state": "ok",
      "enabled": true,
      "vbus_mv": 19020,
      "iout_ma": 430
    },
    "out_b": {
      "state": "ok",
      "enabled": false,
      "vbus_mv": 19010,
      "iout_ma": 0
    }
  },
  "charger": {
    "state": "ok",
    "allow_charge": true,
    "ichg_ma": 520,
    "ibat_ma": 510,
    "vbat_present": true
  },
  "battery": {
    "state": "ok",
    "pack_mv": 15260,
    "current_ma": 180,
    "soc_pct": 67,
    "no_battery": false,
    "discharge_ready": true,
    "issue_detail": null,
    "recovery_pending": false,
    "last_result": null
  },
  "thermal": {
    "tmp_a_state": "ok",
    "tmp_a_c": 39,
    "tmp_b_state": "ok",
    "tmp_b_c": 37
  },
  "network": {
    "state": "connected",
    "ipv4": "192.168.31.42",
    "last_error": null
  }
}
```

- Error: standard error envelope

### 错误（Errors）

- `503/unavailable`: identity not ready（retryable: yes）

### 兼容性与迁移（Compatibility / migration）

- `status` 是后续客户端和 Web 的主要只读 SoT；新增字段应保持向后兼容，不删除现有 key。

## Status Stream（GET `/api/v1/status` + `Accept: text/event-stream`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

- Headers:
  - `Accept: text/event-stream`
- Query: None
- Body: None

### 响应（Response）

- Success:
  - `Content-Type: text/event-stream`
  - `event: status`，`data` 为与普通 `/api/v1/status` 一致的 JSON
  - `event: heartbeat`，`data` 固定为 `{ "ok": true }`
  - 可带 `id: <u32>`

### 错误（Errors）

- `409/unavailable`: status stream already in use（retryable: yes）
- `503/unavailable`: identity not ready（retryable: yes）

### 示例（Examples）

- Request:

```http
GET /api/v1/status HTTP/1.1
Host: mains-aegis-a1b2c3.local
Accept: text/event-stream
```

- Response frame:

```text
id: 1
event: status
data: {"mode":"standby",...}

id: 2
event: heartbeat
data: {"ok":true}
```

### 兼容性与迁移（Compatibility / migration）

- 首版只保证单连接；若后续升级为多订阅广播，应保持事件名和 payload 形状不变。
