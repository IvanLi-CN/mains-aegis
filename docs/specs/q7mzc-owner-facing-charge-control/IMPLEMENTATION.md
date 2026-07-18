# Owner-facing Charge Control Implementation（#q7mzc）

- 设备本体 HTTP、devd device HTTP 与 USB CDC 已统一支持 `charge-control` detail / preview / action 合同。
- `status.charge_control` 收缩为紧凑摘要；Web 详情读取和控制反馈转移到正式 detail payload。
- Power 页移除常驻手动充电表单与常驻合同大表；手动充电改为单弹窗 preview/start/stop/confirm 流。
- Web 侧不再使用 SOC、`diag-snapshot` 或 transport 错误去推断阻断原因。
