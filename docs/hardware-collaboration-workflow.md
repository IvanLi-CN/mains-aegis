# Hardware Collaboration Workflow

本项目的真机协作默认通过 `mcu-agentd` 执行，目标是在多设备、多 worktree 环境下避免连错设备，同时保留 USB CDC / Web Serial 的开发体验。

## Boundaries

- Agent 不直接运行 `espflash`、`cargo espflash` 或 `cargo-espflash`。
- Agent 不枚举候选端口，不运行 `mcu-agentd selector list <MCU_ID>`，也不扫描 `/dev/*`。
- Agent 不切换端口，不运行 `mcu-agentd selector set <MCU_ID> <PORT>`。
- Agent 可以运行非枚举、非切换的 `mcu-agentd` 操作，包括 `config validate`、`mcu list`、`selector get`、`flash`、`monitor`、`logs`、`reset`。
- USB CDC 同一时刻只有一个消费者：`mcu-agentd monitor` 与 Web App Web Serial 不能并发占用。

## One-Time Human Setup

每个 worktree 首次真机协作前，由人类在仓库根目录绑定目标设备：

```bash
mcu-agentd selector list esp
mcu-agentd selector set esp /dev/cu.usbmodemXXXX
mcu-agentd selector get esp
```

绑定结果写入 `firmware/.esp32-port`。如果 `selector get` 缺失，Agent 必须停止真机操作并提示先完成绑定。

## Agent Validation Sequence

Agent 接管真机验证时按以下顺序执行：

1. 校验配置：

   ```bash
   mcu-agentd --non-interactive config validate
   mcu-agentd --non-interactive mcu list
   mcu-agentd --non-interactive selector get esp
   ```

2. 构建目标固件。USB CDC / Web Serial 验证使用：

   ```bash
   cd firmware
   MAINS_AEGIS_WIFI_SSID=usb-placeholder \
     MAINS_AEGIS_WIFI_PSK=usb-placeholder \
     cargo +esp build --release --features web_serial,net_http
   ```

3. 烧录：

   ```bash
   mcu-agentd --non-interactive flash esp
   ```

4. 监视启动日志：

   ```bash
   mcu-agentd --non-interactive monitor esp --reset
   ```

5. 进入 Web Serial 验证前，停止 `monitor`，释放 CDC 口。

6. 启动 Web App 预览并打开 `/connect`。浏览器 Web Serial 设备选择器需要人类选择 CDC 设备；Agent 不替代人类操作系统级 USB 选择器。

7. Web App 验证项：

   - 连接后收到 `hello` 与 identity/capabilities。
   - 设备详情能刷新 `UpsStatus` 与 `NetworkSummary`。
   - structured `log` 面板能显示 USB session 日志。
   - WiFi SSID/PSK set 或 clear 返回 ack/error；PSK 提交后不回显。
   - 断开 Web App 后，可以重新使用 `mcu-agentd monitor esp`。

## Decision Summary

每次设备相关操作都在对话中给出：

```text
Operation type: read-only | state-changing | write
Command: <exact command>
Decision: allow | deny
Rationale: <G0-G4 gate result>
Next step: <next action>
```

## Failure Handling

- `E_SELECTOR_MISSING`：停止，要求人类完成 selector 绑定。
- `E_RESOURCE_BUSY`：报告占用者，不自动抢占。
- 浏览器提示 port already open：确认 `mcu-agentd monitor` 已停止，再重新连接 Web Serial。
- Web Serial permission denied：重新点击连接并由人类在浏览器授权；Agent 不绕过浏览器权限模型。
- WiFi PSK 不写入命令行、不写入日志、不截图展示；只在 Web App 表单中提交给设备。
