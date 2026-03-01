# 前面板 UI 设计（当前确定口径）

本文件用于沉淀当前已确认的前面板 UI 设计结论，优先服务实现、联调与评审阅读。

## 1. 界面范围（先划清边界）

- **固件屏幕界面（已实现）**：详细设计拆分到独立文档：
  - Dashboard：[dashboard-design.md](dashboard-design.md)
  - Self-check：[self-check-design.md](self-check-design.md)
- **上位机界面（未来实现）**：当前仅做范围占位，不包含交互、信息架构或视觉定稿。

## 2. 设计基线

- 运行语义基线：[../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md](../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md)
- 视觉冻结基线：[../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md](../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md)
- 分辨率固定：`320x172`（横屏有效区）
- 当前 Dashboard 基线：`Variant B (Neutral)`
- 当前 Self-check 基线：`Variant C`

## 3. 固件屏幕全局运行语义（来自 #7n4qd）

- 开机后屏幕可用即进入 `SELF CHECK`（Variant C）。
- 自检阶段按模块探测进度实时更新状态：`PEND -> OK/WARN/ERR/N/A`。
- 自检完成后保持 `SELF CHECK` 页面并持续刷新真实运行数据。
- 本版本禁用 `CENTER` 长按切页，不再从自检页切回 Dashboard。

## 4. 固件屏幕文档拆分入口

- Dashboard 模块设计：[dashboard-design.md](dashboard-design.md)
- Self-check 模块设计：[self-check-design.md](self-check-design.md)

## 5. 固件屏幕渲染图预览（文档内显示）

### 5.1 模块分区图

![Dashboard Variant B Module Map](assets/dashboard-b-module-map.png)
![Self-check Variant C Module Map](assets/self-check-c-module-map.png)

### 5.2 冻结渲染图

![Dashboard Variant B - BYPASS](assets/dashboard-b-off-mode.png)
![Dashboard Variant B - STANDBY](assets/dashboard-b-standby-mode.png)
![Dashboard Variant B - ASSIST](assets/dashboard-b-supplement-mode.png)
![Dashboard Variant B - BACKUP](assets/dashboard-b-backup-mode.png)
![Self-check Variant C - STANDBY idle](assets/self-check-c-standby-idle.png)
![Self-check Variant C - STANDBY charger-focus](assets/self-check-c-standby-right.png)
![Self-check Variant C - ASSIST output-focus](assets/self-check-c-assist-up.png)
![Self-check Variant C - BACKUP irq-focus](assets/self-check-c-backup-touch.png)

## 6. 上位机界面（未来实现占位）

- 当前仓库仅冻结固件屏幕界面，不包含上位机 UI 实现。
- 后续新增上位机界面时，建议在 `firmware/ui/` 下新增独立文档（例如 `host-app-design.md`），避免与固件屏幕语义混淆。

## 7. 非当前展示资产

以下图片属于历史参考，不纳入当前确定展示集：

- `docs/specs/6qrjs-front-panel-industrial-ui-preview/assets/dashboard-b-ac-mode.png`
- `docs/specs/6qrjs-front-panel-industrial-ui-preview/assets/dashboard-b-batt-mode.png`

## 8. 追溯来源

- [../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md](../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md)
- [../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md](../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md)
