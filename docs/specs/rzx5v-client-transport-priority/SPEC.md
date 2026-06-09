# Client transport priority matrix（#rzx5v）

## 状态

- Status: 已完成
- Created: 2026-06-08
- Last: 2026-06-08

## 背景 / 问题陈述

- `Web App`、`mains-aegis-devd` 与 `mains-aegis` CLI 会同时面对两层优先级：
  - 客户端入口优先级：先通过 `devd`、direct HTTP，还是 `Web Serial` 进入。
  - 入口背后的硬件 transport / endpoint 优先级：例如 LAN 侧的 `hostname_fqdn > hostname > ip:port`，或 devd 侧的 `USB-first`。
- 这些优先级属于跨客户端共享的底层连接规则，不应散落在 LAN、Web 或 devd 单一专题规格里。
- 若没有单一真相源，Web 默认直连、devd owner path、CLI `--transport`、companion-LAN confirm 后的 endpoint 选择就会在多份规格里各写一套，最终相互冲突。

## 目标 / 非目标

### Goals

- 为 `Web App`、`mains-aegis-devd` 与 `mains-aegis` CLI 提供单一的客户端 × 通信方案优先级矩阵。
- 把“客户端到设备/daemon 的通信方案优先级”和“LAN endpoint 回退顺序”统一落在同一份 owner-facing topic spec 中。
- 冻结 remembered device 必须保存的 per-channel 连接记忆字段，避免“上次用哪条方案连接成功”在不同客户端各记一套。
- 为其他专题 spec 提供引用目标：Web、LAN convergence、devd、CLI 不再各自定义一套优先级表。

### Non-goals

- 不重写设备本体 API 契约。
- 不改变 companion-LAN 的发现、确认或持久化结构。
- 不把 Web lease、devd session、trace retention 等运行态细节搬进本规格。

## 范围（Scope）

### In scope

- `Web App`、`mains-aegis-devd`、`mains-aegis` CLI 的客户端入口优先级。
- companion-LAN 已确认后的 LAN endpoint 回退顺序。
- devd 内部硬件 transport 优先级。
- remembered device 需要保存的连接方式列表与 per-channel 时间字段。
- `pending companion 不参与自动路由`、`identity 复核` 等跨客户端硬规则。

### Out of scope

- 设备扫描策略、scan trace、mDNS/DNS-SD 发现流程。
- Web 组件布局、按钮文案、视觉证据。
- devd HTTP service 托管、IPC/HTTP 命令面、具体 CLI flags 全量定义。

## 需求（Requirements）

### MUST

- 通信方案优先级必须由本规格中的列表顺序定义。
- 本规格必须显式列出所有 owner-facing 连接方式。
- 至少需要两组顺序定义：
  - 客户端入口优先级
  - 入口背后的 transport / endpoint 优先级
- 客户端入口优先级必须覆盖 `Web App`、`mains-aegis-devd` 与 `mains-aegis` CLI。
- remembered device 必须按连接方式分别保存：
  - `last_connected_at`
  - `last_connect_attempt_at`
- `Web App` 不得把 `pending companion_lan_candidate` 作为默认直连路径。
- `mains-aegis-devd` 与 `mains-aegis` CLI 必须保持 `USB-first`。
- 所有进入优先级矩阵的 LAN 地址都必须先通过 `GET /api/v1/identity` 复核，并与目标 logical device 的 `device_id` 一致。

### SHOULD

- 其他专题 spec 应引用本规格，而不是重新定义优先级矩阵。
- companion-LAN 已确认后的 Web direct HTTP 应优先使用 FQDN，再回退到 `hostname`，最后回退到 `ip:port`。

## 功能与行为规格（Functional / Behavior Spec）

### 连接方式清单

owner-facing 客户端连接方式固定为：

1. `devd`
   - `Web App` hosted / self-hosted 通过 devd HTTP service 与 devd Web lease / usb-http bridge 使用
   - `mains-aegis` CLI 通过 devd IPC 使用
2. `direct HTTP`
   - `http://<hostname_fqdn>`
   - `http://<hostname>`
   - `http://<ip>:<port>`
3. `Web Serial`
   - 仅 `Web App` 独立浏览器 / Vite 开发路径

物理 transport 术语固定为：

1. `USB`
2. `LAN`

### remembered device per-channel 字段

remembered device 应沿用 `DeviceTarget.rememberedChannels` 这一层级，并扩展成如下 schema：

```ts
type RememberedChannels = {
  http?: {
    baseUrl: string;
    mdnsHost?: string;
    fallbackBaseUrl?: string;
    source?: "manual" | "devd_discovery";
    seenAt: string;
    last_connected_at?: string;       // new
    last_connect_attempt_at?: string; // new
  };
  devd?: {
    baseUrl: string;
    devdDeviceId?: string | null;
    transport?: "usb" | "lan" | "mock";
    seenAt: string;
    last_connected_at?: string;       // new
    last_connect_attempt_at?: string; // new
  };
  serial?: {
    seenAt: string;
    last_connected_at?: string;       // new
    last_connect_attempt_at?: string; // new
  };
};
```

示例：

```json
{
  "deviceId": "mains-aegis-a1b2c3",
  "transport": "http",
  "preferredTransport": "http",
  "rememberedChannels": {
    "http": {
      "baseUrl": "http://mains-aegis-a1b2c3.local",
      "mdnsHost": "mains-aegis-a1b2c3.local",
      "fallbackBaseUrl": "http://192.168.31.42:80",
      "source": "devd_discovery",
      "seenAt": "2026-06-08T10:20:30Z",
      "last_connected_at": "2026-06-08T10:21:02Z",
      "last_connect_attempt_at": "2026-06-08T10:21:02Z"
    },
    "devd": {
      "baseUrl": "http://127.0.0.1:30080",
      "devdDeviceId": "serial-04f3bb3f5367",
      "transport": "usb",
      "seenAt": "2026-06-08T10:18:00Z",
      "last_connected_at": "2026-06-08T10:19:11Z",
      "last_connect_attempt_at": "2026-06-08T10:19:11Z"
    },
    "serial": {
      "seenAt": "2026-06-07T15:03:10Z",
      "last_connected_at": "2026-06-07T15:05:44Z",
      "last_connect_attempt_at": "2026-06-08T09:58:12Z"
    }
  }
}
```

默认优先级规则：

1. 所有客户端默认优先使用“上次成功连接的方案”。
2. 若不存在 `last_connected_at`，则按本规格定义的入口顺序回退。
3. `last_connect_attempt_at` 只用于诊断、排序和最近尝试可见性；它本身不得覆盖 `last_connected_at` 的优先级。

### 客户端入口优先级

1. `Web App`
   - 优先 1：上次成功连接的 remembered channel
   - 优先 2：若不存在成功历史，但显式设置了 `preferredTransport`，使用 `preferredTransport` 对应 channel
   - 优先 3：若仍无法决定，使用 `devd`
   - 优先 4：若无 `devd`，使用 `Web Serial` 或 direct HTTP
   - 说明：confirmed companion 会把 `preferredTransport` 设为 `http`；独立浏览器 / Vite Web Serial 会把 `preferredTransport` 设为 `serial`
2. `mains-aegis-devd`
   - 优先 1：上次成功连接的硬件 transport
   - 优先 2：若不存在成功历史，使用 `USB`
   - 优先 3：若显式切换或 USB 不可用，使用 `LAN`
3. `mains-aegis` CLI
   - 优先 1：`devd IPC`
   - 说明：CLI 不直接连硬件，所有 USB / LAN 选择都委托给 devd

### Transport priority matrix

在客户端入口已经确定之后，effective hardware transport / LAN endpoint 按下表自上而下选择。若存在显式用户选择或 `last_connected_at` 成功记录，则只要该方案仍满足本表要求的 `bound` / `verified` 条件，就可以优先命中对应行；否则继续向下回退。

| Priority | Communication scheme | Web App | `mains-aegis-devd` | `mains-aegis` CLI |
| --- | --- | --- | --- | --- |
| 1 | Bound USB | 不适用于 direct browser path；hosted / self-hosted Web 只能通过 devd Web lease 使用 bound USB | 只要 USB 已绑定且可用，就是默认 active owner transport | CLI 入口始终是 `devd IPC`；当 devd 可用 bound USB 时，这也是 CLI 的默认硬件 transport |
| 2 | Verified LAN name (`hostname_fqdn`; if unavailable, separately verified `hostname`) | confirmed companion 或显式 direct HTTP 时的默认 HTTP base URL | 显式走 LAN companion 时的首选 endpoint | CLI 通过 devd 走 LAN 时的首选 endpoint |
| 3 | Verified `IP:Port` | direct HTTP fallback URL | LAN fallback endpoint | CLI 通过 devd 走 LAN 时的 fallback endpoint |

### Matrix rules

- LAN endpoint 只有在 `GET /api/v1/identity` 返回的 `device_id` 与目标 logical device 一致时，才算 `verified`。
- `pending companion_lan_candidate` 不参与本矩阵；它只能作为 owner-facing 提示存在，直到用户显式确认。
- 若 companion-LAN identity 校验进入 `lan_identity_conflict`，则第 2、3 行全部阻断，只允许保留 USB 路径。
- `Web Serial` 仍是 `Web App` 的 owner-facing 入口方式，但它不属于 LAN endpoint 行；只有在独立浏览器 / Vite 路径且入口已选定 `serial` 时才使用。
- 本矩阵取代任何旧表述中“Web 默认优先 `IP:Port` 而非已验证 FQDN/mDNS”的说法。

### Per-client interpretation

1. `Web App`
   - 当上次成功 channel 是 `http`，或 `preferredTransport=http` 时，HTTP base URL 顺序固定为：`http://<hostname_fqdn>` -> `http://<hostname>` -> `http://<ip>:<port>`。
   - 当上次成功 channel 是 `serial`，或 `preferredTransport=serial` 时，使用 `Web Serial`。
2. `mains-aegis-devd`
   - 若上次成功 transport 是 `USB`，默认 owner path 仍为 `USB`。
   - 若上次成功 transport 是 `LAN`，或用户显式切到 LAN，则 endpoint 顺序固定为：`http://<hostname_fqdn>` -> `http://<hostname>` -> `http://<ip>:<port>`。
   - 若不存在成功历史，默认 owner path 是 `USB`。
3. `mains-aegis` CLI
   - 入口始终是 `devd IPC`，不是 direct HTTP 或 direct USB。
   - 当 CLI 通过 devd 走 LAN 时，沿用 devd 的第 2、3 行规则；当 CLI 通过 devd 走 USB 时，沿用第 1 行 `USB-first` 规则。

### 硬规则

- `Web App`
  - hosted / self-hosted 场景下，若存在 remembered channels，默认先按各 channel 的 `last_connected_at` 选最近成功方案。
  - 只有在 companion-LAN 已确认后，才允许把 direct HTTP LAN 路径写成默认 `preferredTransport=http`。
  - `pending companion_lan_candidate` 只用于提示，不参与自动路由。
  - `Web Serial` 仍然是正式支持的客户端连接方式，但只属于独立浏览器 / Vite 开发路径；hosted devd UI 走 `devd`。
  - `USB` 是物理 transport，不与 `Web Serial` 并列成 owner-facing 客户端连接方式。
- `mains-aegis-devd`
  - 若无连接历史，默认 owner path 是 `USB`。
  - 显式切到 LAN 时，内部 endpoint 顺序固定为 `hostname_fqdn > hostname > ip:port`。
- `mains-aegis` CLI
  - CLI 的入口永远是 `devd IPC`，不是 direct HTTP 或 direct USB。
  - CLI 通过 `devd` 使用硬件；因此 CLI 的 `USB-first` 体现为 `devd` 内部默认 owner path 是 `USB`。
  - 显式选 `usb` 且 USB 不可用时直接失败，不自动降级到 LAN。

## 验收标准（Acceptance Criteria）

- 存在一份独立 topic spec，把跨客户端通信方案优先级从 LAN / Web / devd 专题规格中抽离。
- 客户端入口优先级必须正确体现：
  - `Web App` 支持 `devd`、direct HTTP、`Web Serial`
  - `mains-aegis` CLI 通过 `devd IPC` 通信
- 本规格必须显式列出 owner-facing 连接方式清单。
- remembered device 规范必须要求每种 remembered channel 都保存 `last_connected_at` 与 `last_connect_attempt_at`。
- LAN / 硬件 transport 优先级必须覆盖 `hostname_fqdn`、`hostname`、`ip:port`、`USB` 四类路径，并区分“客户端连接方式”与“物理 transport”。
- `Web App`、`mains-aegis-devd`、`mains-aegis` CLI 的 owner-facing 规格都引用本规格，而不再重复定义矩阵。
- 本规格与现有实现一致：
  - `Web App` 当前实现仍以 `preferredTransport` / `devd` 作为主排序
  - confirmed companion 设定 `preferredTransport=http` 后，direct HTTP 顺序为 `hostname_fqdn > hostname > ip:port`
  - CLI 通过 `devd IPC` 通信
  - devd 内部保持 `USB-first`

## 文档更新（Docs to Update）

- `docs/specs/k4vzn-lan-management-convergence/SPEC.md`
- `docs/specs/7jqrq-mains-aegis-cli-devd-alignment/SPEC.md`
- `docs/specs/p8k3d-mains-aegis-devd/SPEC.md`
- `docs/specs/ypfpu-web-management-ui/SPEC.md`

## 参考（References）

- `docs/specs/k4vzn-lan-management-convergence/SPEC.md`
- `docs/specs/7jqrq-mains-aegis-cli-devd-alignment/SPEC.md`
- `docs/specs/p8k3d-mains-aegis-devd/SPEC.md`
- `docs/specs/ypfpu-web-management-ui/SPEC.md`
