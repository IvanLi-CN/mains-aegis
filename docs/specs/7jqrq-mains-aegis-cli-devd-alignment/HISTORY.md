# History（#7jqrq）

## 2026-06-02

- 决定采用 host-tools single-crate 结构：`tools/mains-aegis-host` 产出 CLI 与 devd。
- 决定 hard switch：`serve` 只提供 IPC，HTTP/Web 改为显式 `bridge-http`。
- 决定用户操作必须依赖 release 安装工具；源码构建保留给开发 skill。
