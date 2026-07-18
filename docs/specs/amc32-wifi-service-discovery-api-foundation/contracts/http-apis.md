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

## Settings（GET `/api/v1/settings`）

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: none

### 请求（Request）

- Headers: None
- Query: None
- Body: None

### 响应（Response）

- Success schema:

```json
{
  "wifi": {
    "configured": true,
    "ssid": "LabNet"
  },
  "log_level": "info",
  "manual_charge": {
    "target": "full_100",
    "speed": "ma_500",
    "timer_h": 2
  },
  "advanced_power": {
    "standby_drop_mv": 1200,
    "assist_low_drop_mv": 600,
    "assist_enter_delta_ma": 0,
    "assist_exit_delta_ma": 0,
    "assist_required_samples": 2,
    "assist_ramp_step_mv": 100,
    "assist_ramp_interval_ms": 200,
    "rated_enter_delta_ma": 0,
    "rated_exit_delta_ma": 0,
    "vin_drop_threshold_pct": 4,
    "required_samples": 2
  },
  "advanced_power_capabilities": {
    "rated_vout_mv": 12000,
    "standby_drop_mv": { "default": 1200, "min": 0, "max": 3000, "step": 20 },
    "assist_low_drop_mv": { "default": 600, "min": 0, "max": 3000, "step": 20 },
    "assist_enter_delta_ma": { "default": 0, "min": -100, "max": 1000, "step": 50 },
    "assist_exit_delta_ma": { "default": 0, "min": -50, "max": 1000, "step": 50 },
    "assist_required_samples": { "default": 2, "min": 1, "max": 5, "step": 1 },
    "assist_ramp_step_mv": { "default": 100, "min": 20, "max": 1000, "step": 20 },
    "assist_ramp_interval_ms": { "default": 200, "min": 100, "max": 3000, "step": 100 },
    "rated_enter_delta_ma": { "default": 0, "min": -100, "max": 1000, "step": 50 },
    "rated_exit_delta_ma": { "default": 0, "min": -50, "max": 1000, "step": 50 },
    "vin_drop_threshold_pct": { "default": 4, "min": 1, "max": 12, "step": 1 },
    "required_samples": { "default": 2, "min": 1, "max": 5, "step": 1 }
  }
}
```

- `wifi.ssid` 允许为 `null`；PSK 永远不得出现在返回中。
- `advanced_power` 仅返回相对额定输出的偏移/阈值语义，不返回 owner-facing 可写的绝对输出值。
- `advanced_power_capabilities.rated_vout_mv` 是解释偏移量的基线；UI/CLI 必须以 capabilities 提供的 `default/min/max/step` 作为唯一显示与校验真相源。

### 错误（Errors）

- None（仅传输层错误）

### 兼容性与迁移（Compatibility / migration）

- `settings` 是设备本体当前可管理设置的完整快照；后续可新增字段，但不得把返回拆成多份零散读取端点。

## WiFi Config（POST/DELETE `/api/v1/wifi-config`）

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: none

### 请求（Request）

- `POST /api/v1/wifi-config`

```json
{
  "ssid": "LabNet",
  "psk": "correct horse"
}
```

- `DELETE /api/v1/wifi-config`
- Query: None

### 响应（Response）

- Success: `202 Accepted`

```json
{"accepted":true}
```

- 语义：请求被设备 HTTP worker 接收后进入主循环串行执行队列；真实写入沿用 USB CDC WiFi config 的 EEPROM 与运行时 WiFi 更新路径。
- PSK 不得出现在响应、日志或 trace payload 中。

### 错误（Errors）

- `400/invalid_wifi_ssid`: SSID 非法（retryable: no）
- `400/invalid_wifi_psk`: PSK 非法（retryable: no）
- `409/busy`: 已有未消费 LAN management command（retryable: yes）

## Log Level（POST `/api/v1/settings/log-level`）

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: none

### 请求（Request）

```json
{"level":"info"}
```

### 响应（Response）

- Success: `202 Accepted`

```json
{"accepted":true}
```

### 错误（Errors）

- `400/invalid_log_level`: level 不在 `error|warn|info|debug|trace` 中（retryable: no）
- `409/busy`: 已有未消费 LAN management command（retryable: yes）

## Manual Charge（POST `/api/v1/settings/manual-charge`）

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: none

### 请求（Request）

```json
{
  "target": "rsoc_80",
  "speed": "ma_500",
  "timer_h": 2,
  "power_path": "auto"
}
```

- `target`: `pack_3v7 | rsoc_80 | full_100`
- `speed`: `ma_100 | ma_500 | ma_1000`
- `timer_h`: `1 | 2 | 6`
- `power_path`: `auto | dcin | usbc`

### 响应（Response）

- Success: `202 Accepted`

```json
{"accepted":true}
```

### 错误（Errors）

- `400/invalid_manual_charge_prefs`: 参数不在安全集合中（retryable: no）
- `409/busy`: 已有未消费 LAN management command（retryable: yes）

## Charge Control（GET `/api/v1/charge-control`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 响应（Response）

- Success: `200 OK`

返回 owner-facing 详情对象：

```json
{
  "summary": {},
  "readiness": {},
  "telemetry": {},
  "evidence": []
}
```

- `summary`: 当前模式、会话剩余时间、loop override 活跃态
- `readiness`: `state/action/planned_path/block/loop_override`
- `telemetry`: 当前输入源、策略目标、IBAT 实测、目标电压、IINDPM/VINDPM、输出功率与限流摘要
- `evidence`: 固件正式导出的直接证据数组

## Charge Control Preview（POST `/api/v1/charge-control/preview`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

```json
{
  "target": "full_100",
  "current_ma": 500,
  "timer_minutes": 120,
  "power_path": "auto"
}
```

### 响应（Response）

- Success: `200 OK`

返回与 `GET /api/v1/charge-control` 同形状的 detail，用于回答“如果现在点 START 会发生什么”。

## Manual Charge Control（POST `/api/v1/control/manual-charge`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

```json
{"action":"start","confirm_loop":false}
```

- `action`: `start | stop`
- `confirm_loop`: 仅用于 USB-C loop confirmation 场景

### 响应（Response）

- Success: `200 OK`

返回与 `GET /api/v1/charge-control` 同形状的 detail。

### 错误（Errors）

- 正式失败也必须返回同形状 detail 于 `error.details`
- `loop_confirmation_required` 必须以 `readiness.state=confirm_required` 表达

## Advanced Power（POST `/api/v1/settings/advanced-power`）

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: none

### 请求（Request）

```json
{
  "standby_drop_mv": 1200,
  "assist_low_drop_mv": 600,
  "assist_enter_delta_ma": 0,
  "assist_exit_delta_ma": 0,
  "assist_required_samples": 2,
  "assist_ramp_step_mv": 100,
  "assist_ramp_interval_ms": 200,
  "rated_enter_delta_ma": 0,
  "rated_exit_delta_ma": 0,
  "vin_drop_threshold_pct": 4,
  "required_samples": 2
}
```

- 所有字段都按整块替换语义写入。
- `standby_drop_mv`、`assist_low_drop_mv` 为相对 `rated_vout_mv` 的 `mV drop`。
- `assist_enter_delta_ma`、`assist_exit_delta_ma` 为相对 `assist_low` 默认门槛的 `mA delta`。
- `assist_required_samples` 为 `assist_low` 进入/退出锁存窗口。
- `assist_ramp_step_mv`、`assist_ramp_interval_ms` 只服务 `standby -> assist_low` 的限速爬升。
- `rated_enter_delta_ma`、`rated_exit_delta_ma` 为相对设备默认门槛的 `mA delta`。
- 不暴露任何 owner-facing 可写绝对 `VIN` 门槛；`assist_low` 入口绝对 `VIN` 比较只存在于运行时内部。

### 响应（Response）

- Success: `202 Accepted`

```json
{"accepted":true}
```

### 错误（Errors）

- `400/invalid_advanced_power_settings`: 步进、范围或跨字段关系不合法（retryable: no）
  - 至少包括：
    - `standby_drop_mv >= assist_low_drop_mv >= 0`
    - 展开后 `assist_exit_threshold_ma <= assist_enter_threshold_ma`
    - 展开后 `rated_exit_threshold_ma <= rated_enter_threshold_ma`
- `409/busy`: 已有未消费 LAN management command（retryable: yes）

## Advanced Power Reset（POST `/api/v1/settings/advanced-power/reset`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

```json
{}
```

### 响应（Response）

- Success: `202 Accepted`

```json
{"accepted":true}
```

## Reset（POST `/api/v1/reset`）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

```json
{"confirm":"reset"}
```

### 响应（Response）

- Success: `202 Accepted`

```json
{"accepted":true}
```

- 语义：设备主循环消费命令后短暂延迟并执行 software reset。调用方必须预期 HTTP 连接和 LAN 可达性中断。

### 错误（Errors）

- `400/missing_field`: 缺少 `confirm`（retryable: no）
- `400/unsafe_operation`: `confirm` 不是 `reset`（retryable: no）
- `409/busy`: 已有未消费 LAN management command（retryable: yes）

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
    "cell_mv": [3812, 3817, 3809, 3822],
    "cell_delta_mv": 13,
    "balance_enabled": true,
    "balance_cfg_match": true,
    "balance_active": true,
    "balance_mask": 10,
    "balance_cell": null,
    "balance_min_start_delta_mv": 3,
    "no_battery": false,
    "discharge_ready": true,
    "charge_fet_on": true,
    "discharge_fet_on": true,
    "precharge_fet_on": false,
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
