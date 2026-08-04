# History（client-transport-priority）

## 2026-06-08

- 新建独立 topic spec，把跨客户端通信方案优先级从 LAN / Web / devd 专题规格中抽离。
- 冻结当前 owner-facing 规则：
  - Web confirmed companion: `hostname_fqdn > hostname > ip:port`
  - devd / CLI: `USB-first`
- 把 LAN fallback 规则收口成单一 `client × communication scheme` 矩阵，并明确它取代旧的 “Web 默认优先 `IP:Port`” 说法。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
