# Hardware Collaboration Workflow

本项目的真机协作默认通过 released `mains-aegis` / `mains-aegis-devd` 执行，目标是在多设备、多 worktree 环境下避免连错设备，同时保留 USB CDC / Web Serial 的开发体验。

## Boundaries

- Agent 不直接运行 `espflash`、`cargo espflash` 或 `cargo-espflash`。
- Agent 不使用 `mcu-agentd` 作为 Mains Aegis 设备操作路径。
- Agent 不枚举 `/dev/*` 或其它串口路径。
- Agent 不切换端口，也不“换一个端口试试”。
- `mains-aegis-devd` 可以执行 owner-visible scan/list/bind/connect 流程；scan 只发现候选设备，不自动连接或切换。
- USB CDC 同一时刻只有一个消费者：`mains-aegis-devd` 持有设备时，Web App Web Serial 不能并发占用同一 CDC 口。

## One-Time Human Setup

每个 worktree 首次真机协作前，通过 released host tools 绑定目标设备：

```bash
mains-aegis-devd serve
mains-aegis devices scan
mains-aegis device <device-id> bind --alias <name>
mains-aegis device <device-id> connect
mains-aegis device <device-id> identity
```

如果本轮需要 HTTP/Web 或 `power-diag`，从一开始使用 `bridge-http` 代替 `serve`；不要在 `serve` 仍占用默认 IPC endpoint 时再启动默认 `bridge-http`：

```bash
mains-aegis-devd bridge-http --allow-dev-cors
mains-aegis devices scan
mains-aegis device <device-id> bind --alias <name>
mains-aegis device <device-id> connect
```

绑定结果由 `mains-aegis-devd` 持久化到用户配置态。若没有已知绑定或无法确认 identity，Agent 必须停止真机操作并提示先完成 owner-visible 绑定。

## Agent Validation Sequence

Agent 接管真机验证时按以下顺序执行：

1. 校验配置：

   ```bash
   mains-aegis --version
   mains-aegis-devd --version
   mains-aegis devices list
   ```

2. 构建目标固件。USB CDC / Web Serial 验证使用：

   ```bash
   cd firmware
   cargo +esp build --release
   ```

3. 烧录必须通过 devd 绑定设备和 Firmware Catalog artifact 执行；真实烧录需要 owner authorization，默认先 dry-run：

   ```bash
   mains-aegis device <device-id> artifact select --manifest-path <manifest.json>
   mains-aegis device <device-id> flash --dry-run
   ```

4. 监视启动日志必须通过 devd 绑定设备执行；真实 monitor 需要已知绑定设备：

   ```bash
   mains-aegis device <device-id> monitor start
   mains-aegis device <device-id> trace
   mains-aegis device <device-id> monitor stop
   ```

5. 读取充电/电源状态使用 devd HTTP bridge 的只读 `power-diag` API：

   ```bash
   curl http://127.0.0.1:30080/api/v1/devices/<device-id>/power-diag
   ```

   如果当前只启动了 IPC-only `serve`，先停止该 daemon，再以 `bridge-http --allow-dev-cors` 重新启动同一 IPC endpoint；不要让两个进程同时绑定默认 IPC endpoint。

6. 进入 Web Serial 验证前，断开 devd 设备连接或停止当前 devd bridge，释放 CDC 口：

   ```bash
   mains-aegis device <device-id> disconnect
   ```

7. 启动 Web App 预览并打开 `/connect`。浏览器 Web Serial 设备选择器需要人类选择 CDC 设备；Agent 不替代人类操作系统级 USB 选择器。

8. Web App 验证项：

   - 连接后收到 `hello` 与 identity/capabilities。
   - 设备详情能刷新 `UpsStatus` 与 `NetworkSummary`。
   - structured `log` 面板能显示 USB session 日志。
   - WiFi SSID/PSK set 或 clear 返回 ack/error；PSK 提交后不回显。
   - 断开 Web App 后，可以重新通过 devd 连接同一设备。

## Decision Summary

每次设备相关操作都在对话中给出：

```text
Operation type: read-only | state-changing | write
Command: <exact command>
Decision: allow | deny
Rationale: <G0-G5 gate result>
Next step: <next action>
```

## Failure Handling

- `E_RESOURCE_BUSY`：报告占用者，不自动抢占。
- 浏览器提示 port already open：确认当前项目 devd 已断开设备或停止 bridge，再重新连接 Web Serial。
- Web Serial permission denied：重新点击连接并由人类在浏览器授权；Agent 不绕过浏览器权限模型。
- WiFi PSK 不写入命令行、不写入日志、不截图展示；只在 Web App 表单中提交给设备。
