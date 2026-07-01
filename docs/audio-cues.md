# 音效对照与预览

本页集中列出当前固件/试听资产使用的提示音，便于对照触发语义与听感。

## 交互操作音

本轮选定 `B. Warm Tap`：

- 有效触摸 / 有效按键：操作被成功识别，且触发页面、路由、选择、偏好、弹层状态变化，或产生对应业务 `UiAction`。空白触摸、未定义目标、重复按键且状态未变化不发声。
- USB-C 插入：USB-PD attach 从未连接切换为已连接时发声；PD 协商刷新、contract 更新或保持态不重复发声。

| ID | 用途 | 路由 | 预览 |
| --- | --- | --- | --- |
| `interaction_touch` | 有效触摸 / 有效按键 | `ACTION` | <audio controls src="./audio-cues-preview/interaction-feedback/audio/set_b_touch.wav"></audio> |
| `usb_c_insert` | USB-C 插入 | `ACTION` | <audio controls src="./audio-cues-preview/interaction-feedback/audio/set_b_usb_c_insert.wav"></audio> |

交互操作音预览页：[`audio-cues-preview/interaction-feedback/preview.html`](./audio-cues-preview/interaction-feedback/preview.html)

## 状态 / 告警 / 错误音

既有 `speaker_chime_v1` 运行时提示音保持原语义：状态音单次播放，告警音按 `2000ms` 间隔循环，错误音连续循环。

| ID | 标题 | 分类 | 触发语义 | 预览 |
| --- | --- | --- | --- | --- |
| `boot_startup` | 开机音 | status | 系统上电启动成功后触发 | <audio controls src="./audio-cues-preview/audio/boot_startup.wav"></audio> |
| `mains_present_dc` | 市电出现音（仅DC桶） | status | 检测到 DC 桶输入恢复时触发 | <audio controls src="./audio-cues-preview/audio/mains_present_dc.wav"></audio> |
| `charge_started` | 充电开始音 | status | 充电状态从未充电切换为充电中时触发 | <audio controls src="./audio-cues-preview/audio/charge_started.wav"></audio> |
| `charge_completed` | 充电完成音 | status | 充电状态进入完成态时触发 | <audio controls src="./audio-cues-preview/audio/charge_completed.wav"></audio> |
| `shutdown_mode_entered` | 进入关闭模式音 | status | 系统进入关闭模式流程时触发；当前主固件保持 dormant | <audio controls src="./audio-cues-preview/audio/shutdown_mode_entered.wav"></audio> |
| `mains_absent_dc` | 市电不存在告警（仅DC桶） | warning | DC 桶输入丢失时触发间隔循环 | <audio controls src="./audio-cues-preview/audio/mains_absent_dc.wav"></audio> |
| `high_stress` | 压力大告警 | warning | 任一模块温度/负载不佳但未触发保护时触发间隔循环 | <audio controls src="./audio-cues-preview/audio/high_stress.wav"></audio> |
| `battery_low_no_mains` | 电池电量低告警（无市电） | warning | 电池低电且市电不存在时触发间隔循环 | <audio controls src="./audio-cues-preview/audio/battery_low_no_mains.wav"></audio> |
| `battery_low_with_mains` | 电池电量低告警（有市电） | warning | 电池低电且检测到市电时触发间隔循环 | <audio controls src="./audio-cues-preview/audio/battery_low_with_mains.wav"></audio> |
| `shutdown_protection` | 停机保护错误 | error | 任一模块触发保护动作导致停机时连续循环 | <audio controls src="./audio-cues-preview/audio/shutdown_protection.wav"></audio> |
| `io_over_voltage` | 输入输出过压错误 | error | 输入或输出检测到过压时连续循环 | <audio controls src="./audio-cues-preview/audio/io_over_voltage.wav"></audio> |
| `io_over_current` | 输入输出过流错误 | error | 输入或输出检测到过流时连续循环 | <audio controls src="./audio-cues-preview/audio/io_over_current.wav"></audio> |
| `io_over_power` | 输入输出过功率错误 | error | 输入或输出检测到过功率时连续循环；当前主固件保持 dormant | <audio controls src="./audio-cues-preview/audio/io_over_power.wav"></audio> |
| `module_fault` | 模块故障错误 | error | 部分硬件通信失败期间连续循环 | <audio controls src="./audio-cues-preview/audio/module_fault.wav"></audio> |
| `battery_protection` | 电池保护错误 | error | BMS 触发保护时连续循环 | <audio controls src="./audio-cues-preview/audio/battery_protection.wav"></audio> |

状态/告警/错误增强预览页：[`audio-cues-preview/preview.html`](./audio-cues-preview/preview.html)
