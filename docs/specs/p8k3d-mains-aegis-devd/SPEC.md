# Mains Aegis Device Daemon（#p8k3d）

## 状态

- Status: 已完成（v1 devd foundation）
- Created: 2026-05-02
- Last: 2026-06-07

## 接管说明

- 本规格记录的是 `mains-aegis-devd` v1 foundation。
- 其中 `session`、`/api/v1/serial/session`、localhost settings 兼容面，以及 devd 如何接入设备本体 LAN 管理 API，已转由 [`#k4vzn`](../k4vzn-lan-management-convergence/SPEC.md) 重设计。
- 本规格保留 devd 作为本机 USB owner、host power、artifact/flash/reset/monitor foundation 的历史基线。

## 背景 / 问题陈述

`mcu-agentd` 曾承担烧录、reset 与 defmt monitor，但 Web App 的 USB CDC 业务通信也需要独占同一 USB Serial/JTAG CDC 口。多个进程同时抢串口会导致日志、配网、状态读取和烧录互相干扰。

项目需要一个独立于 `mcu-agentd` 的本地设备入口：它可以管理多个硬件、为 Web UI 提供 HTTP/SSE API、托管生产 Web 静态资源，并用统一 Firmware Catalog 处理烧录与 defmt artifact 匹配。

## 目标 / 非目标

### Goals

- 新增 Mains Aegis 专用设备 daemon；当前 canonical host-tools crate 为 `tools/mains-aegis-host`（见 #7jqrq）。
- `serve` 启动不接收设备端口；设备通过 API 扫描、列出、绑定、连接、断开和解绑。
- 设备绑定、别名和已选择 artifact 属于用户配置态，必须持久化到 devd 状态文件；默认位置复用参考项目的 host-tools 模式：`directories::ProjectDirs::config_dir()` 下的 `devices.json`。daemon 重启后 `GET /api/v1/devices` 仍能返回已知绑定，后续 `scan` 会把当前可见端口重新附加到对应绑定。
- HTTP API 覆盖 identity、connection、settings、trace、events、artifact selection、reset、monitor start/stop、flash 与设备 settings 写入；`session` 仅作为历史兼容语义由 #k4vzn 接管。
- HTTP API 覆盖 host power 查询、低功耗运行 profile 切换、suspend、shutdown dry-run 与事件广播。
- 吸收旧本地 USB HTTP bridge 的兼容面：WiFi config、log level、manual charge endpoints 与 Web USB Console hydration 由 devd 直接提供；新的 owner-facing 查询面使用 `connection / settings / trace`。
- 跨 `Web App` / `mains-aegis-devd` / `mains-aegis` CLI 的通信方案优先级矩阵由 [`#rzx5v`](../rzx5v-client-transport-priority/SPEC.md) 统一定义；本规格不再重复定义跨客户端默认 transport 规则。
- Firmware Catalog 成为 Web Direct、devd、本地构建和 GitHub Release 的统一 artifact 合同。
- 固件 identity 暴露 build/profile/features/protocol/defmt 信息，devd 用它与 artifact manifest 匹配；不匹配时日志解码必须标记 `unverified`。
- Web 开发期由 Vite dev server 反代 `/api` 到显式启动的 `serve-http --allow-dev-cors`，proxy target 可由 env 指向当前开发实例；hosted 模式下由 devd 直接托管嵌入式 Web。需要浏览器直接跨源访问 devd API 时，`--allow-dev-cors` 只允许 loopback HTTP development origins。Connect 页在 LAN 发现结果里只保留 `identity.firmware.protocol === "mains-aegis.cdc.v1"` 的候选；hosted Connect 中这些 LAN 候选只作为 direct hardware HTTP target 使用，不应伪装成 `devd transport`。
- 新增项目 skill，固化 devd 设备操作、安全边界和验证流程；Codex 在本仓内默认使用 `$mains-aegis-devd-flow` 做开发、验证、诊断与硬件 read/session-read 检查，`$mains-aegis-user-operations` 仅用于显式 end-user/released-tool 场景。

### Non-goals

- 第一版不实现浏览器端完整 ESP ROM 烧录；Web Direct flash 只保留 catalog/client 边界。
- 不在本规格中清理历史 `mcu-agentd.toml` 文件；`mcu-agentd` 不作为 Agent 的 Mains Aegis 设备操作路径。
- 不优化多设备并发烧录；v1 使用 per-device 状态与安全串行模型。
- 不在无硬件环境执行真机烧录、reset 或 monitor。
- 不把 suspend/sleep 归类为低功耗运行；低功耗运行必须保持主机 awake，devd 和监听程序仍可继续工作。

## 功能规格

### devd API

- `GET /api/v1/devices`: 返回当前已知设备与绑定。
- `POST /api/v1/devices/scan`: 枚举本机 serial candidates，只发现不自动连接。
- `POST /api/v1/devices/{id}/bind`: 为已知设备创建稳定绑定与别名，并写入 devd 持久状态；当 USB identity 尚未可读但 owner 已知其对应的 logical device 时，请求可显式携带 `logical_device_id`，把该 stable USB id 绑定到已有 logical device。
- `POST /api/v1/devices/{id}/connect`: 连接设备并读取/缓存 identity。
- `POST /api/v1/devices/{id}/disconnect`: 断开设备 session。
- `DELETE /api/v1/devices/{id}/binding`: 移除绑定，并同步 devd 持久状态。
- `GET /api/v1/devices/{id}/identity`: 返回设备 firmware identity。
- `GET /api/v1/devices/{id}/power-diag`: 通过 USB CDC `get_power_diag` 获取只读电源诊断快照，并缓存到设备 session。
- `GET|POST /api/v1/devices/{id}/artifact`: 查询或选择 artifact manifest。
- `POST /api/v1/devices/{id}/flash`: 校验 artifact hash 后执行烧录；无硬件验证使用 `dry_run=true`。真实烧录响应与 `flash completed` 事件必须同时回传 backend `status/stdout/stderr`，用于区分“artifact 选择正确但底层 flash backend 没有真正完成”和“backend 已成功写入硬件”。真实 flash backend 必须有明确超时并在超时路径清理子进程，避免 HTTP 客户端断开后遗留卡住的底层烧录进程。
- `POST /api/v1/devices/{id}/reset`: 设备 reset 请求；native serial 后端必须在已绑定端口上执行 in-process DTR/RTS app-boot 复位，保持 boot 释放线为实测 app-boot 电平，不再另起外部 reset 进程抢占同一串口。
- `POST /api/v1/devices/{id}/monitor/start|stop`: monitor 生命周期请求；native serial monitor attach 不隐式复位，避免 USB 复位后继续持有重枚举前的串口 fd；需要复位时必须先通过 `/reset` 关闭式脉冲控制线，再重新 start monitor。
- `GET /api/v1/devices/{id}/connection`: 返回 transport、连接状态、绑定与 artifact 上下文。
- `GET /api/v1/devices/{id}/settings`: 返回当前设备 settings 快照。
- `GET /api/v1/devices/{id}/trace`: 返回 bounded logs/trace 与 `log_decode`。
- `GET /api/v1/devices/{id}/trace` 必须允许 owner 通过客户端 follow 轮询看到新增 `kind=event,target=power` 的 synthetic power event；event payload 至少包含 `event/input_source/pressure_state/pressure_reason/pressure_score_pct/vin_vbus_mv/vin_baseline_mv/policy_target_ichg_ma/limit_reason/tps_total_iout_ma/tps_limit_threshold_ma`。
- `GET /api/v1/devices/{id}/events`: 设备事件 SSE。
- 设备状态边沿变化时，devd 必须把 power-related 边沿变化收敛为 single-shot synthetic event，而不是每次 poll 刷新都广播重复事件。
- `POST /api/v1/wifi-config` / `DELETE /api/v1/wifi-config`: 通过指定 `device_id` 的已连接设备写入或清除 WiFi 配置；未指定 `device_id` 时仅允许单设备连接场景。
- `POST /api/v1/settings/log-level`: 通过指定 `device_id` 的连接设备更新日志级别。
- `POST /api/v1/settings/manual-charge`: 通过指定 `device_id` 的连接设备更新手动充电偏好。

`power-diag` 响应必须保持只读，不触发充电策略、BMS 恢复或输出状态变化。快照至少包含：

- `input`: DC IN/VIN、charger-side input ADC、USB-C attach/VBUS/contract/unsafe-source latch。
- `charger`: BQ25792 enable/control pin state、charge/input policy gates、status/fault raw bytes、decoded `CHG_STAT/VBUS_STAT/ICO_STAT` 与 ADC values；必须同时暴露 `vac1_adc_mv` 与 `vac2_adc_mv`，用于区分 USB-C VAC1 与 DC IN/VAC2 实际输入路径；还必须暴露 `vbat_lowv_pct_x10` 与 `iprechg_ma`，确认低压恢复的 `REG08` 已写为 `71.4% / 120mA`。
- `policy`: `allow_charge=false` 或 `vbat_present=false` 相关的 policy state/status/notice、input source、target charge current、output-load and manual-charge blockers；低压恢复时必须暴露 `recovery_stage=bq40_pchg|bq25792_precharge`。
- `bms`: BQ40Z50 pack/current/RSOC/cell range、RCA、charge/discharge readiness、raw safety/PF/manufacturing/gauging/operation status、XCHG/CHG/DSG/PCHG/FET enable/CUV/CUVC/charging inhibit flags；必须暴露 `cuv_recovery_mv` 与 `cuv_recov_chg`，确认维护 DF baseline 是否已应用。

### Host power control

devd 的 host power 控制面用于 UPS 后备供电期间降低主机负载，并在电量不足时触发优雅关机流程。`low_power_running` 是主机仍然 awake/running 的节能 profile 状态，不等同于 suspend/sleep；sleep 后 devd 本身会暂停，不能继续承担 UPS 协调。

- `GET /api/v1/host/power`: 返回平台后端、是否允许真实动作、dry-run 默认值、能力、当前 power profile、保存的 previous profile 与最近一次 host power action。
- `POST /api/v1/host/power/profile`: 请求切换主机 power profile。请求体为 `{ "profile": "power_saver|balanced|performance|restore_previous", "dry_run": true }`；不支持的 profile 返回 API-compatible error envelope。
- `POST /api/v1/host/power/suspend`: 请求主机进入 suspend/sleep。该动作独立于低功耗运行，默认只做 dry-run。
- `POST /api/v1/host/power/shutdown`: 请求主机关机。请求体为 `{ "delay_sec": 60, "dry_run": true, "force": false }`；真实关机还必须携带 `confirm:"shutdown"`。`force:true` 只表示上游 Web App/UPS 已明确要求强制服从断电窗口，devd 不自行推断。
- `GET /api/v1/host/power/events`: 返回 `host_power` SSE 事件，供本机监听程序得知 profile 切换、suspend、shutdown dry-run、真实执行请求和拒绝原因。

安全规则：

- 所有 state-changing host power 请求默认 `dry_run=true`；未显式开启真实动作时，`dry_run=false` 必须返回 `host_power_real_action_denied`。
- 真实动作只在 devd 启动时带 `--allow-host-power-actions` 或环境变量 `MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS=true` 时允许。
- dry-run 响应必须包含 backend、action、target profile 或 delay、以及将执行的命令摘要，并且必须广播 `host_power` 事件。
- shutdown `delay_sec` 按秒解释；真实关机请求必须立即下发给操作系统并以系统命令返回码作为 API 结果，不得由 devd 自行计时或自行决定何时关机。`delay_sec=0` 表示立即关机；`delay_sec>0` 表示使用系统级关机调度能力。
- Linux 后端使用 `power-profiles-daemon` 的 `net.hadess.PowerProfiles` D-Bus `ActiveProfile` 作为低功耗运行入口：`power_saver` 映射为 `power-saver`，`balanced` 与 `performance` 映射到同名 profile；suspend 使用 logind D-Bus，shutdown 使用 `systemctl poweroff --no-block --when=...`，并仅在请求带 `force:true` 时附加 `--force`，确保命令一旦接收就由系统执行。
- Linux `force:true` 仅支持 `delay_sec=0`；`force:true` 与非零延迟无法由 systemd 同时可靠表达，必须返回 `host_power_shutdown_unsupported`，由上游明确改发立即强制关机或非强制延迟关机。
- macOS 后端使用 `pmset lowpowermode 1/0` 进入/退出低功耗运行；suspend 使用 `pmset sleepnow`，shutdown 使用系统 `shutdown`。macOS 原生命令只支持分钟粒度调度，`delay_sec>0` 会向上取整为分钟。
- macOS 后端不接受 `force:true`；该 backend 无法用 `pmset`/`shutdown` 表达强制关机语义，必须返回 `host_power_shutdown_unsupported`，避免将普通 shutdown 误报为 forced compliance。
- 若平台缺少 `power-profiles-daemon`、`busctl`、`pmset` 或权限不足，API 必须返回可诊断错误，不得 panic。

UPS 策略建议：

- 市电掉电并进入后备供电时，优先调用 profile dry-run/真实切换至 `power_saver` 并广播 `host_power` 事件，让监听程序降低负载。
- 市电恢复后，调用 `restore_previous` 或 `balanced` 恢复正常运行 profile。
- 电量不足时，由 Web App 或 UPS 控制程序按自己的策略决定何时发出真实 shutdown；devd 不做额外关机决策。真实 shutdown 成功发起后即可认为指令已被系统接收；若 UPS 断电窗口要求系统必须立即服从，上游必须发送 `delay_sec:0, force:true`。suspend 只用于允许暂停业务的主机场景，不等同于可断电的关机/休眠。

### Host power VM validation

CI 必须覆盖 devd 真实命令触发路径，而不仅是 fake command 或 dry-run：

- Linux 使用 GitHub Actions `ubuntu-latest` runner 启动 QEMU Ubuntu guest VM，在 guest 内安装 `power-profiles-daemon`，以真实 `dry_run=false` 请求验证 `power_saver` profile 生效，并向 guest 下发真实 shutdown，断言 VM 关机退出。
- macOS 使用 GitHub Actions `macos-latest` 受管 runner VM 执行真实 `pmset lowpowermode` 切换，并发起可取消的系统 scheduled shutdown，断言 macOS 接收关机命令。若 GitHub-hosted macOS runner 不暴露 `lowpowermode`，该 job 必须验证 profile 请求返回可诊断 error envelope，并继续验证真实 scheduled shutdown。GitHub-hosted macOS runner 不支持在 runner 内再启动 macOS nested VM，因此该 job 以 runner VM 本身作为 macOS VM 验证对象。

### Web USB control lease

devd 的 Web 控制面必须以显式 Web session 租约作为 USB 占用依据。设备连接不能因为扫描、页面探活或存在历史连接记录而长期保留；只有当前 Web 页面持有有效租约时，devd 才能占用对应 USB CDC 设备。

- `connection`、Web lease、monitor handle、logs 和 trace 是运行态，不写入持久状态；daemon 重启后这些状态必须安全回到 disconnected / no lease，避免伪造仍连接的硬件 session。
- 持久状态只保存用户意图：设备绑定、别名、最近可见端口路径、已加载 artifact manifest 和每个设备选择的 artifact id。端口路径是最近观测值，`scan` 负责刷新或清空，不得触发自动连接。

- Web 连接流程必须是 `scan -> owner selects device -> lease/connect -> heartbeat -> release/expiry`。
- `scan` 可以列出多个 USB CDC candidates，但不得自动选择或自动连接任何 candidate。
- 多个 native serial candidates 存在时，devd 必须把完整候选列表返回给 Web；Web 必须让用户明确选择要控制的设备。devd 和 Web 都不得基于 “已识别 / 已连接 / 第一个 / 最近使用” 自动替用户决定。
- Web 创建租约时必须提交用户选择的 devd device id；devd 仅能连接该指定设备。目标不存在、不可连接、被其他有效租约占用或 identity 不可用时返回 API-compatible error envelope。
- 租约创建成功后，devd 返回 `lease_id`、`device_id`、`identity.device_id`、`expires_at`、`heartbeat_interval_ms` 与 `lease_ttl_ms`。
- settings、WiFi config、log level、manual charge、USB Console hydration、serial event stream 等 Web USB 控制请求必须携带有效 `lease_id` 或绑定到有效 lease；无有效租约时返回 `web_session_required` 或 `web_session_expired`，不得继续写入硬件。
- 正常释放路径：Web 在显式 disconnect、移除设备、页面 `pagehide` / `beforeunload` 时应使用 keepalive request 或 `sendBeacon` 发送 release；devd 收到 release 后必须立即停止 monitor、关闭 native serial session，并把设备状态更新为 disconnected。
- 异常释放路径：Web 断网、浏览器崩溃、系统休眠或网络抖动导致 release 未送达时，devd 通过租约 TTL 自动释放。默认目标为 `heartbeat_interval_ms=2000`、`lease_ttl_ms=8000`、cleanup tick 不超过 `1000ms`；因此无心跳后通常应在 8-9 秒内释放 USB 占用，不允许分钟级错误占用。
- 网络抖动处理：单次 SSE 断开、短暂 heartbeat 失败或页面短暂不可见不得立即释放；只要 heartbeat 在 TTL 内恢复，devd 保持租约。超过 TTL 后释放，后续 Web 必须重新创建租约并重新读取 identity。
- 租约是 per-device exclusive。一个设备同一时间最多一个有效 Web lease；多个 Web 页面竞争同一设备时，后来的请求返回 `device_lease_conflict`，除非用户显式在 UI 中释放旧 lease 后重试。
- flash/reset/monitor 等非 Web 页面直连操作仍必须遵守设备 guardrails；如果这些操作需要占用同一 native serial port，devd 必须先拒绝或释放不兼容的 Web lease，且错误信息要能让 Web 显示“设备正被其它操作占用”。

### Multi-device selection contract

- `POST /api/v1/devices/scan` 的响应必须保留每个 candidate 的 devd `id`、`display_name`、`port_path`、`connection`、`binding` 与可用的 `identity`；当某个 USB candidate 已通过 `bind` 绑定到已有 logical device 时，`binding.logical_device_id` 必须原样返回，供 Web 在 identity pending 阶段仍可把该 USB candidate 归并回正确设备。
- Web 只可在用户选择某个 candidate 后调用 lease/connect；候选数量为 0 时显示无设备，候选数量大于 1 时显示选择器，不得要求用户物理拔掉其它设备作为常规路径。
- devd 兼容 root-level `/api/v1/identity`、`/api/v1/status`、`/api/v1/network` 只能在存在唯一有效 Web lease 或请求明确带 `device_id/lease_id` 时返回设备数据。否则返回 `device_selection_required`，避免多设备场景误读错误硬件。

### Firmware Catalog

- Canonical schema: `schemas/firmware-catalog.schema.json`。
- 本地生成脚本: `tools/firmware-artifact/build-catalog-entry.py`。
- Web fallback catalog: `web/public/firmware/firmware-catalog.json`。
- 每个 artifact 记录 `artifact_id/name/version/git_sha/build_id/target_chip/profile/features/protocol/defmt/files`。
- `files[].sha256` 必须在 devd flash 前重新校验。

### 固件身份

- HTTP identity 与 USB CDC `hello/get_identity` 的 `firmware` 字段必须包含：
  - `package_version`
  - `build_profile`
  - `build_id`
  - `git_sha`
  - `src_hash`
  - `git_dirty`
  - `features`
  - `protocol`
  - `defmt`
- devd 匹配规则：只有 `build_id`、build profile 与 feature set 都和 selected artifact 精确匹配才可标记 `log_decode.status=verified`；`git_sha` 只能作为 provenance 展示，不能单独证明 defmt artifact 匹配。

## 验收标准

- `tools/mains-aegis-host` 能编译并通过单元测试。
- devd 可无端口启动；无硬件验证通过 dry-run API 与单元测试中的 synthetic in-memory device state 覆盖设备管理、artifact selection、dry-run flash 与 session API。
- devd 重启后仍保留绑定、别名和 artifact selection；但不会恢复 connected/Web lease/monitor/log ring 等运行态。
- host power API 支持 Linux/macOS 查询、dry-run、事件广播和真实动作默认拒绝；缺少平台后端或权限不足时返回可诊断错误。
- `tools/firmware-artifact/build-catalog-entry.py` 能为 ELF 生成 manifest、catalog 和 `SHA256SUMS`。
- 固件 identity JSON 包含 features/protocol/defmt 字段。
- 固件 USB CDC 支持 `get_power_diag`，devd `GET /api/v1/devices/{id}/power-diag` 能返回并缓存结构化 `input/charger/policy/bms` 诊断快照。
- Given DC IN 与 USB-C 同时在线，When charger 实际 VBUS/VAC2 为约 12V 且 VAC1 为约 5V，Then `power-diag` 必须能同时呈现 `input.input_source=dcin`、`charger.vac2_adc_mv≈12V`、`charger.vac1_adc_mv≈5V`、`charger.iindpm_ma=1000`，即使 `charger.vbus_stat` 仍报告 USB SDP 类枚举值。
- Given BMS 处于 CUV 低电恢复且 `BQ25792 CHG_STAT=termination_done`，When 读取 `power-diag`，Then `policy.state` 必须保持 `recovering_low_voltage`、`policy.status=RECOV`、`policy.recovery_stage=bq40_pchg|bq25792_precharge`、`policy.full_latched=false`，不得把该快照误报为满充锁存。
- Given charger poll 已完成，When 读取 `power-diag`，Then `charger.vbat_lowv_pct_x10=714`、`charger.iprechg_ma=120` 可见。
- Given BQ40 DF 可读，When 读取 `power-diag`，Then `bms.cuv_recovery_mv` 与 `bms.cuv_recov_chg` 可见，用于确认 `2550mV + CUV_RECOV_CHG=0` baseline。
- Web typecheck 通过，且 dev server proxy 将 `/api` 反代到 env 指定或默认的 `serve-http --allow-dev-cors`。
- 文档与 AGENTS guardrails 清晰说明 `$mains-aegis-devd-flow` 是本仓 Codex 默认入口；显式 end-user/released-tool 操作才使用 `$mains-aegis-user-operations`。
- 多 USB CDC 设备同时存在时，devd/Web 不自动选择；Web 显示候选列表，用户选择后才创建 Web lease 并占用设备。
- Web 正常断开后 devd 立即释放 USB 占用；Web 异常断开后 devd 按租约 TTL 自动释放，默认目标不超过 9 秒。
- Web USB 写入请求缺少有效 lease 时失败，不得因为 devd 里有历史 connected 设备而继续写硬件。
- Given `POST /api/v1/devices/{id}/flash` 触发真实烧录，When backend 返回成功或失败，Then HTTP 响应与设备事件都必须包含 backend `status/stdout/stderr`，便于定位 `espflash` 是否真正完成。
- Given `POST /api/v1/devices/{id}/flash` 触发真实烧录，When backend 在超时窗口内没有返回，Then devd 必须返回可诊断的 retryable `espflash_timeout`，并确保 backend 子进程不会继续作为活动烧录流程悬挂。
- Given `POST /api/v1/devices/{id}/flash` 触发 ESP32-S3 USB 烧录，When flash 写入完成，Then backend 优先使用 `watchdog-reset` after-operation，避免 DTR/RTS normal reset 在当前样机上被 strap 采样为 ROM download。
- Given native serial `reset` 占用已绑定端口，When devd 需要让 ESP32-S3 运行 app，Then devd 必须用自身 serial handle 执行 boot-release、RTS pulse、boot-release 的 app-boot 控制线序列，不得通过额外进程重新打开端口；monitor/start 不得在已打开 monitor fd 上重复执行该复位序列。
- 低压恢复 HIL 必须可通过 `tools/hil/low-voltage-recovery.sh` 完成“`tools/bq40-comm-tool` 临时固件 apply DF -> devd 烧回主固件 -> USB `power-diag` 验证”的双烧录流程；runner 必须拒绝非批准 device id / port，并在 real 模式下阻断任何指向 `/dev/cu.usbmodem212101` 的绑定。

## 实现状态

- `tools/mains-aegis-host`: v1 daemon/API/mock validation foundation，并提供 CLI、IPC 与显式 HTTP service。
- `web/`: hosted Connect 只显示 devd discovery；devd 列出的 USB 设备通过 lease/usb-http bridge 进入 Web，LAN 设备则在 Web 中直接落为硬件 HTTP record。
- `tools/mains-aegis-host`: 提供设备级 `power-diag` 只读诊断 API，转发固件 USB CDC `get_power_diag` 并在 session 中缓存结果。
- `tools/mains-aegis-host`: flash API 与 `flash completed` 事件已暴露 backend `status/stdout/stderr`，用于现场确认底层 `espflash` 执行结果。
- `tools/mains-aegis-host`: flash backend 有可配置超时，默认避免底层烧录进程在 HTTP 客户端超时或断开后无界悬挂。
- `tools/mains-aegis-host`: flash backend 使用 `watchdog-reset` after-operation，避免当前 ESP32-S3 USB CDC 样机 post-flash 被 DTR/RTS line reset 拉进 ROM download。
- `tools/mains-aegis-host`: native serial reset 使用 in-process DTR/RTS app-boot 复位，并保持 boot 释放线为实测 app-boot 电平；monitor attach 不隐式复位也不主动改写 DTR/RTS，且 monitor 已运行时 `/reset` 复用 monitor 线程持有的串口，避免外部 reset 进程和重枚举前 monitor fd 争抢同一串口。
- `firmware/src/net_contract.rs`: `power-diag.charger` 已暴露 `vac2_adc_mv`，用于定位 BQ25792 AC2/DC IN 实际采样。
- `tools/mains-aegis-host`: 提供 host power control surface；低功耗运行、suspend、shutdown 默认 dry-run，真实动作受启动参数保护。
- `schemas/firmware-catalog.schema.json`: v1 catalog schema。
- `tools/firmware-artifact/build-catalog-entry.py`: local manifest/catalog generator。
- `web/src/api/*`: devd mode client contracts。
- `firmware/src/net_contract.rs`: firmware identity/status/power diagnostic JSON contract。

## 变更记录（Change log）

- 2026-06-14: `power event`、`status` 与 `power-diag` 统一补充 `tps_total_iout_ma` / `tps_limit_threshold_ma`，用于解释 `pressure_tps_output_current`；DC IN profile 的 `iindpm_ma` 基线更新为 `1000mA`。
- 2026-06-04: `power-diag` 增加 `charger.vbat_lowv_pct_x10`、`charger.iprechg_ma`、`policy.recovery_stage`、`bms.cuv_recovery_mv` 与 `bms.cuv_recov_chg`，支持确认 `REG08=71.4%/120mA` 与 BQ40 `2550mV + CUV_RECOV_CHG=0` baseline。
- 2026-06-04: `flash` API 与设备事件增加 backend `status/stdout/stderr` 透传，现场可直接确认 `espflash` 是否真正完成以及目标硬件 identity 是否已经切到新 artifact。
- 2026-06-04: 新增低压恢复 HIL runner 与文档，固化 bq40 工具固件和主固件的双烧录验证路径。
- 2026-06-05: `/api/v1/status` 的 `battery` snapshot 增加四节 `cell_mv`、`cell_delta_mv`、均衡状态字段与 `charge_fet_on` / `discharge_fet_on` / `precharge_fet_on`，Web 电池页可直接展示 per-cell voltage、delta、BAL 状态与三路 BMS MOS 状态，不再依赖 `power-diag` 详情端点。
- 2026-06-07: `devices/scan` 与 `devices` 响应中的 `binding.logical_device_id` 成为 Web 归并 USB identity-pending candidate 与 Fleet 混合视图的 canonical 键；旧绑定若缺失该字段，Connect 仍可继续显式补绑到已有 logical device。
