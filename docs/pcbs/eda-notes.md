# EDA / 网表备忘

本页用于记录 EDA 工程与网表导出中容易踩坑的细节，方便后续复核与脚本处理。

## 1. `.enet` 格式（EasyEDA 导出 JSON）

- `.enet` 是 JSON；顶层通常只有 `version/components/designRule/...`，不一定存在单独的 `nets` 列表。
- 网络名里可能包含 `$`（例如 `$1N59`），写脚本/正则时不要把 `$` 当成特殊字符。
- 未连网脚位常表现为 `"net": ""`；有些器件会对“可选脚”不给 `NC` 命名（例如 Type-C 16pin 版本的 `SBUx`/`DPx`/`DNx` 可能为空网），不要默认判为错误。

## 2. Designator 命名漂移

- 不要仅靠 `FPC1/FPC2` 等 designator 推断功能；以 Pin->Net 映射为准。
- 若出现 designator 对调（例如主板口/屏幕口互换），需要同步更新文档，或在原理图里改回约定，避免后续接线/装配误解。

## 3. 浮空网（single-connection net）检查

- 重点检查“只出现一次的 net”（single-connection net），常见于网名误用导致的地参考浮空（例如 `UGND` 未并到 `GND`）。
- 对于 ESD/共模滤波类器件，参考地浮空会直接削弱甚至失效其保护能力。

## 4. ECMF02-2AMX6（共模滤波 + ESD）信号标注

- 器件本体为被动网络，通常不限制方向；但为避免 USB 端到端的 `D+`/`D-` 被交换，建议保持系统网络命名一致性（例如 `DPU` 端到端对应 `UCM_DP`，`DMU` 端到端对应 `UCM_DM`）。
- 数据手册强调：`GND` 连接应尽量短，`NC` 保持不连接。

