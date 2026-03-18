# Dashboard 二级详情页点击钻取（#f3c2g）

## 状态

- Status: 已完成
- Created: 2026-03-15
- Last: 2026-03-17

## 背景 / 问题陈述

- 当前 `Variant B` Dashboard 已改为真实运行态首页，但仍停留在“首页摘要”层级。
- 主页上的 `KPI / info panel / BATTERY / CHARGE / DISCHG` 已具备清晰信息分区，却没有点击钻取能力。
- 新需求要求在不打散现有首页骨架的前提下，为一般用户提供 5 个二级仪表盘页面，先完成界面与交互壳层，再对接真实细粒度数据源。

## 目标 / 非目标

### Goals

- 在 `320x172` 前面板上新增 Dashboard 内部路由：`Home` 与 5 个详情页。
- 首页 5 个固定入口映射：
  - `主 KPI` -> `Output`
  - `次级信息面板` -> `Thermal`
  - `BATTERY` -> `Cells`
  - `CHARGE` -> `Charger`
  - `DISCHG` -> `Battery Flow`
- 详情页统一采用“全屏单页 + 左上 BACK 返回”的结构，不引入滚动、横滑或多级菜单。
- 详情页视觉延续 `Variant B` 的工业仪表基底，但文案、留白和状态标签更偏消费级、可读性更强。
- 未接线字段统一显示 `N/A` 或 `--`，不得伪造演示波动值。
- 通过 `tools/front-panel-preview` 产出首页与 5 个详情页冻结 PNG，作为评审基线。

### Non-goals

- 不在本规格中接入新的 BQ40 单体数据、INA 历史曲线、TMP/Fan 新采样链路。
- 不新增返回 `SELF CHECK` 页入口。
- 不引入图表历史回放、分页容器、滚动列表或趋势线。
- 不改变当前 bitmap 字体白名单、分辨率和主色板基底。

## 范围（Scope）

### In scope

- `firmware/src/front_panel_scene.rs`
  - 新增 Dashboard 路由、触摸命中区与 5 个详情页 renderer。
  - 新增 `DashboardDetailSnapshot` 与详情页 mock/fallback 结构。
- `firmware/src/front_panel.rs`
  - 接入 Dashboard 详情页状态机、触摸进入和返回逻辑。
- `tools/front-panel-preview/src/main.rs`
  - 新增 Dashboard 首页/详情页场景导出。
- `firmware/ui/`
  - 更新 Dashboard 文档与新增详情页冻结文档。

### Out of scope

- `output/mod.rs` 中新增细粒度采样协议解析或实时存储。
- 新增其它屏幕主题或自检页重构。

## 功能与行为规格

### 1. Dashboard 路由

- 新增 `DashboardRoute`：
  - `Home`
  - `Detail(DashboardDetailPage)`
- 新增 `DashboardDetailPage`：
  - `Cells`
  - `BatteryFlow`
  - `Output`
  - `Charger`
  - `Thermal`

### 2. 首页入口映射

- 首页保持现有 `Variant B` 骨架与数据口径。
- 5 个热区使用现有模块几何，不重排主布局：
  - `x=6 y=22 w=196 h=52` -> `Output`
  - `x=6 y=76 w=196 h=94` -> `Thermal`
  - `x=206 y=22 w=108 h=48` -> `Cells`
  - `x=206 y=72 w=108 h=48` -> `Charger`
  - `x=206 y=122 w=108 h=48` -> `BatteryFlow`
- 首页仅增加轻量“可点语义”：活跃边框、角标或提示文案；不改卡片主文案。

### 3. 详情页结构

- 顶栏：`BACK`、页面标题、状态 chip。
- 主体：2~4 个固定信息块，全部单屏可见。
- 底栏：异常/提示条，优先显示 fault/warning，其次显示数据源占位提示。
- 返回规则：
  - 点击左上 `BACK` 返回首页。
  - 详情页状态下按 `LEFT` 或 `CENTER` 视为返回首页。

### 4. 页面口径

- `Cells`
  - 4 节 cell 电压
  - balancing 状态
  - 4 路 cell temp
  - 充/放电状态与异常条
- `Battery Flow`
  - pack voltage / current
  - stored energy / full capacity（mWh）
  - `CHG / DSG / PCHG` gate 状态
  - battery status / faults
- `Output`
  - `VOUT / POUT`
  - `OUT-A / OUT-B` 各自电流与温度
  - 若某个 `TPS` 关闭，其电流固定显示 `--`
  - faults / warning summary
- `Charger`
  - input source（`DC IN` / `USB-C` / `AUTO`）
  - input power
  - charging active
  - charger state / abnormal info
- `Thermal`
  - TMP / board / battery 温度槽位
  - fan level / PWM / tach 状态
  - thermal fault summary

### 5. 空态与异常态

- 缺失值：数值统一 `N/A`。
- 关闭输出路电流：统一 `--`。
- 底部异常条优先级：`FAULT > WARN > SOURCE PENDING > READY`。

## 接口变更（Interfaces）

- `front_panel_scene`
  - 新增 `DashboardRoute`
  - 新增 `DashboardDetailPage`
  - 新增 `DashboardTouchTarget`
  - 新增 `DashboardDetailSnapshot`
- `FrontPanel`
  - 新增 Dashboard 当前 route 状态
  - 运行态输入处理从“Dashboard 无触摸行为”扩展为“首页触摸钻取 + 详情返回”
- `front-panel-preview`
  - 新增 `dashboard-home`
  - 新增 `dashboard-detail-cells`
  - 新增 `dashboard-detail-battery-flow`
  - 新增 `dashboard-detail-output`
  - 新增 `dashboard-detail-charger`
  - 新增 `dashboard-detail-thermal`

## 验收标准（Acceptance Criteria）

- Given Dashboard 首页，When 点击任一入口区，Then 必须进入唯一绑定的详情页。
- Given 任一详情页，When 点击 `BACK` 或按 `LEFT/CENTER`，Then 返回 Dashboard 首页。
- Given `Output` 详情页中某一路 `TPS` 关闭，When 渲染对应电流，Then 显示 `--`。
- Given 详情字段未接入真实数据，When 渲染页面，Then 使用 `N/A` / `--`，且不回落到 demo 波动值。
- Given preview 导出详情页，When 检查图片，Then 每张图均为 `320x172`。
- Given 首页和详情页对比评审，When 观察视觉语言，Then 首页仍可识别为现有 Variant B，详情页明显更易读、更像一般用户仪表盘。

## 实现记录

- 新增 Dashboard 内部路由：`Home` 与 5 个详情页。
- 首页 5 个固定热区已接入触摸钻取，并补上详情页 `BACK` 返回。
- 新增 `DashboardDetailSnapshot` 作为详情页 UI 壳层字段容器；未接线字段统一走 `N/A` / `--`。
- `Output` 详情页把“TPS 关闭时电流显示 `--`”固化为渲染规则。
- 预览工具新增 5 个详情页场景，并已导出冻结 PNG 到 `firmware/ui/assets/` 与本 spec `assets/`。

## 验证记录

- `cargo build --manifest-path /Users/ivan/.codex/worktrees/5efd/mains-aegis/tools/front-panel-preview/Cargo.toml`
- `cargo test --manifest-path /Users/ivan/.codex/worktrees/5efd/mains-aegis/tools/front-panel-preview/Cargo.toml`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/5efd/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-runtime-standby --out-dir /tmp/mains-aegis-dashboard-detail-preview`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/5efd/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-detail-cells --out-dir /tmp/mains-aegis-dashboard-detail-preview`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/5efd/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-detail-battery-flow --out-dir /tmp/mains-aegis-dashboard-detail-preview`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/5efd/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-detail-output --out-dir /tmp/mains-aegis-dashboard-detail-preview`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/5efd/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-detail-charger --out-dir /tmp/mains-aegis-dashboard-detail-preview`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/5efd/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-detail-thermal --out-dir /tmp/mains-aegis-dashboard-detail-preview`
- 说明：`firmware/Cargo.toml` 当前依赖本地路径 `../ina3221-async`，该 worktree 中缺少对应目录，因此无法在本地完成整包 firmware build；本轮已用同源 renderer 的 preview crate 完成构建与单测验证。

## Visual Evidence (PR)

当前冻结版评审图如下，包含首页与 5 个二级详情页：

![Dashboard review set](./assets/dashboard-review-set.png)

### Home

![Dashboard home](./assets/dashboard-home.png)

### Cells

![Dashboard cells detail](./assets/dashboard-detail-cells.png)

### Battery Flow

![Dashboard battery flow detail](./assets/dashboard-detail-battery-flow.png)

### Output

![Dashboard output detail](./assets/dashboard-detail-output.png)

### Charger

![Dashboard charger detail](./assets/dashboard-detail-charger.png)

### Thermal

![Dashboard thermal detail](./assets/dashboard-detail-thermal.png)

### Icons

![Dashboard detail icons](./assets/dashboard-detail-icons.png)
