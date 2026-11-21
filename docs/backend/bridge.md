# 远程访问 Bridge（Relay + E2EE）

> 状态：已实现。桌面 Host、iOS / Android 客户端和自建 Relay 都已实现。只支持 Relay 路径，没有 LAN 直连、多主机或 CLI 常驻 Host。

手机没有本地 Vault。手机扫描桌面生成的二维码后，经公网 Relay 连接桌面 Agentero，读写桌面**当前打开的本地 Vault**，并驱动桌面上的 ACP Agent。Relay 只转发 WebSocket 帧。应用消息全部端到端加密，Relay 看不到 Vault 内容或 Agent 内容。

和 [remote.md](remote.md) 不是一回事：remote 是桌面经 SSH/SFTP 打开服务器上的 Vault；Bridge 是移动端远程使用桌面上的 Vault。远程 Vault（`remote:<sessionId>`）不能开启 Bridge，设置页会禁用开关。产品设计与移动端 UI 见 [../frontend/mobile.md](../frontend/mobile.md)。

## 模块

`src-tauri/src/integration/bridge/`：

| 文件 | 职责 |
|---|---|
| `protocol.rs` | `BridgeOffer`（二维码载荷）与 `agentero://pair#offer=` 编解码、校验；`RelayEndpoint` 解析，拼出 `/ws?v=2&serverId=…&role=…[&connectionId=…]`；`DEFAULT_RELAY_ENDPOINT = de.rune.ing:2054` |
| `identity.rs` | 桌面身份、已配对设备表、客户端设备身份与连接 profile 的读写；Ed25519 挑战签名与验签 |
| `crypto.rs` | `SessionCipher`：`crypto_box` SalsaBox（X25519 + XSalsa20-Poly1305），帧格式 `[24B nonce][密文]` |
| `messages.rs` | 明文握手 `E2eeHandshake`、Relay 控制消息 `RelayControlMessage`、加密应用消息 `BridgeMessage` |
| `relay.rs` | tokio-tungstenite 连接与收发。自动回 WebSocket Pong；非 Normal 的 Close 带码和原因报错 |
| `host.rs` | 桌面 Host：`BridgeController`、控制通道、每个连接一个数据通道、配对、设备认证、RPC 分发、Agent 事件转发 |
| `client.rs` | 移动端客户端：`BridgeClientController`，负责连接循环、配对/认证和 RPC 往返 |
| `commands.rs` | 桌面命令（非 iOS/Android 编译） |
| `client_commands.rs` | 移动端命令（仅 iOS/Android 编译） |

`BridgeController` 和 `BridgeClientController` 都在 `app/mod.rs` 注册为 State。应用退出时，run closure 调用 `BridgeController::stop()` 关闭 Relay 连接。

前端：`src/lib/bridge/host.ts`（桌面，类型化 bindings）、`src/lib/bridge/client.ts`（移动端，字符串 `invoke`）、`src/lib/bridge/pdf.ts` + `file-cache.ts`（PDF 分块下载与 IndexedDB 缓存）。桌面入口是设置页 `src/components/settings/panes/remote-access-pane.tsx`，移动端页面在 `src/components/mobile/`。

## 身份与持久化

所有文件都在 `bridge_config_dir()` = `<XDG 配置目录>/agentero/bridge/`（Unix 默认 `~/.config`，Windows 为 `%APPDATA%`，移动端为 `dirs::config_dir()`）。写入统一走 `write_private_json`：先 `create_dir_all`，再用 owner-only 原子写入替换 JSON；Unix 临时文件从创建起即为 `0600`，Windows 继承目录权限。

| 文件 | 端 | 内容 |
|---|---|---|
| `identity.json` | 桌面 | `{v, serverId, publicKeyB64, secretKeyB64, createdAt}`。`serverId` = `agt_` + UUID 的 base64url；密钥是 X25519，base64url 编码。首次 `bridge_start` 时创建，之后不变 |
| `devices.json` | 桌面 | `BridgeDevice[]`：`{deviceId, name, publicKeyB64, pairedAt, lastSeenAt, revoked}`。吊销只置 `revoked=true`，不删除条目 |
| `client-identity.json` | 移动端（非 iOS） | `{v:2, deviceId:"ios_…", publicKeyB64, secretKeyB64, createdAt}`，Ed25519 设备密钥 |
| （iOS Keychain） | iOS | 客户端身份存在 service `com.poco-ai.agentero.bridge`、account `client-identity-v2` 下。若存在旧的 `client-identity.json`，会迁移进 Keychain 并删除文件 |
| `client-profile.json` | 移动端 | `{offerUrl, deviceName, paired}`，记录最近一台桌面，供 `bridge_resume` 使用 |

身份私钥不会写进二维码，也不会发给 Relay。目前没有「重新生成身份」的命令。

## 二维码 offer

`bridge_offer` 返回 `BridgeOffer` 和 `agentero://pair#offer=<base64url(JSON)>`：

```jsonc
{ "v": 1, "serverId": "agt_…", "hostPublicKeyB64": "<32B base64url>",
  "relay": { "endpoint": "host:port" }, "hostName": "…", "pin": true }
```

offer 不带 token。`from_pair_url` 会校验以下几项：scheme/host 必须是 `agentero://pair`，`v == 1`，`serverId` 以 `agt_` 开头且长度 ≥ 10，Host 公钥为 32 字节，endpoint 合法，`hostName` 非空。Endpoint 不写 scheme 时默认 `wss://`，允许 `ws`/`wss`；路径只能为空或 `/ws`，不能带 query 或 fragment。

## 连接流程

1. **控制通道**：`bridge_start` 先解析 Vault（`resolve_vault`），再加载或创建身份，然后连接 `…/ws?v=2&serverId=…&role=server`。Relay 下发 `sync{connectionIds}` / `connected{connectionId}` 时，Host 为每个 id 建一条数据通道（同一个 id 只建一条）；收到 `disconnected` 时移出活动表。断线后固定 2s 重连。在线状态变化会发 `bridge:host-status`。
2. **数据通道**：URL 与控制通道相同，再加 `&connectionId=…`。
3. **E2EE 握手**（明文 text 帧）：客户端每次连接都生成临时 X25519 密钥，发送 `{"type":"e2ee_hello","key":<标准带填充 Base64>}`。Host 用自己的长期私钥和这个临时公钥建立 `SessionCipher`，回复 `{"type":"e2ee_ready"}`。客户端用临时私钥和 offer 里的 Host 公钥建立同一把 cipher。之后所有应用消息都是加密的 binary 帧。Host 收到无法解密或无法解析的帧时直接结束该数据通道。
4. **首次配对**：客户端发送 `pair_request{requestId, deviceId, deviceName, devicePublicKeyB64}`。Host 校验设备公钥是合法的 Ed25519 公钥，生成 `NNN-NNN` 确认码，存入 pending，发出 `bridge:pair-request`，并给客户端回复 `pair_pending{requestId, verificationCode}`（客户端会转发为 `bridge:pair-pending`）。桌面用户通过 `bridge_pair_respond` 允许或拒绝；300s 内无响应按拒绝处理。允许后 Host 用 `upsert` 写入 `devices.json`（同 `deviceId` 会覆盖旧条目，`revoked` 恢复为 `false`），回复 `pair_ok`，再发 `server_info`。客户端随后把 profile 标记为 `paired`。拒绝、超时或存储失败时回复 `pair_denied{reason}`。
5. **再次连接**：已配对的客户端发 `hello{deviceId, protocolVersion:1, appVersion}`。Host 只对 `devices.json` 中未吊销的设备回复 `device_challenge{nonce_b64}`（32 随机字节）。客户端用 Ed25519 签名后回复 `device_proof`。验签通过后，Host 认证该通道，更新 `lastSeenAt`，并发 `server_info{serverId, hostName, appVersion, vaultName}`。未知设备、已吊销设备或验签失败时 Host 不回复。
6. **RPC**：只有经过第 4 步或第 5 步认证的通道才能调用 RPC。未认证时返回 `rpc_result{ok:false, error:{code:"unauthorized"}}`。请求格式为 `rpc{id, method, params}`，响应为 `rpc_result{id, ok, data?, error?{code, message}}`。另有 `ping{ts}`，Host 回复 `pong{ts}`。

吊销只影响之后的 `hello` 验证。代码不会主动断开已认证的数据通道。客户端没有 RPC 超时：连接关闭或出错时，所有未完成请求一起失败。客户端断线后同样固定 2s 重连。

## RPC 方法

分发入口是 `host.rs::dispatch_rpc`。路径参数都是 Vault 相对路径，经 `vault_relative_path` 解析（见下节）。未列出的方法一律返回 `Bridge RPC method is not allowed`。

| 方法 | 参数 | 语义 |
|---|---|---|
| `vault_tree_build` | — | 构建整棵 Vault 树（`tree::build_tree`，带 CapsCache） |
| `vault_search` | `query`, `limit?` | 在当前 Vault 中全文搜索（`search::vault_search`） |
| `vault_read_text` | `path` | 返回 UTF-8 文本全文 |
| `vault_write_text` | `path`, `content` | 用原子替换写入文本，返回 `null` |
| `bridge_file_info` | `path` | 返回 `{path, size, modifiedAt(ms), sha256}`，`path` 为 `/` 分隔的 Vault 相对路径 |
| `bridge_paper_pdf_info` | `paperPath` | 论文目录第一层按路径排序后的第一个 `.pdf`（扩展名不区分大小写），返回 file info；目录中没有 PDF 时报错 |
| `bridge_read_bytes` | `path`, `offset`, `len?` | 读取一个分块，`len` 取值 1..=256 KiB（默认 256 KiB）。返回 `{file, offset, bytesB64}`（base64url）；`offset` 超过文件大小时报错 |
| `paper_list` | — | `papers::list_all`，返回 Catalog 全部论文 |
| `paper_get` | `path` 或 `id` | 按路径或 id 取一条 Catalog 记录 |
| `paper_set_is_read` | `path`, `isRead` | 设置已读状态 |
| `paper_set_tags` | `path`, `tags` | 替换论文标签 |
| `agent_list_agents` / `agent_scan_catalog` | — | 列出已配置 Agent / 扫描可安装 Agent 目录 |
| `agent_ensure_catalog` | `templateId`, `setDefault?` | 按模板确保 Agent 已登记 |
| `agent_probe_catalog` | `templateId` | 探测模板 Agent 是否可用 |
| `agent_run_once` | `RunOnceRequest` | 在桌面主窗口（没有主窗口时取任一窗口）发起一轮 Agent 运行。`vaultPath` 强制为 Host Vault，`hideFromChatHistory = false` |
| `agent_cancel_run` | `sessionId` | 取消运行 |
| `agent_respond_permission` / `agent_respond_elicitation` / `agent_respond_ask_user` | 对应 Request | 回复权限请求、elicitation 和 ask-user |
| `agent_list_sessions` | `agentId?`, `cursor?` | 分页列出该 Vault 的历史会话 |
| `agent_load_session` / `agent_delete_session` | `sessionId`, `agentId?` | 恢复会话 / 删除会话 |

`agent_*` 系列复用 `features/agent/service.rs`，结果经 `ApiResult` 包络后取出 `data`。在 iOS 构建里，Host 的 `agent_*` 一律返回错误。

**Agent 事件转发**：数据通道监听 `agent:stream`、`agent:completed`、`agent:failed`、`agent:tool`、`agent:plan`、`agent:usage`、`agent:status`、`agent:permission-request`。只有 payload 中的 `sessionId` 属于本通道 `agent_run_once` / `agent_load_session` 返回过的会话时，才以 `event{name, payload}` 推给客户端。客户端再以 `bridge:event:{name}` 重新发出。

## 路径约束（`vault_relative_path`）

- 输入先 trim。以下输入一律拒绝：空串、绝对路径，以及含 `Normal` / `CurDir` 以外组件（`..`、根、盘符前缀）的路径。
- Vault 根先 `canonicalize`。
- 读（`writing=false`）：对完整候选路径 `canonicalize`，所以目标必须存在，符号链接会被解析。
- 写（`writing=true`）：只 `canonicalize` 父目录，再拼上文件名。父目录必须已存在，代码不会建目录。
- 解析结果必须 `starts_with` 规范化后的根，否则报 `Bridge path escapes the Vault`。`file_info` 还会再做一次 `strip_prefix` 校验，并要求目标是文件。

## Tauri 命令与事件

桌面（导出到 `src/lib/core/bindings.ts`，事件在 `events_contract.rs` 登记）：

| 命令 | 说明 |
|---|---|
| `bridge_start({vaultPath, hostName, relayEndpoint?})` | 启动 Host（会先停掉已有实例）。`relayEndpoint` 为空时用默认值，设置页固定传 `de.rune.ing:2054` |
| `bridge_stop` / `bridge_status` | 停止 / 读取 `BridgeStatus`（`enabled, online, serverId, relayEndpoint, hostName, vaultPath, activeConnections, pendingPairings, lastError`） |
| `bridge_offer` | 返回 `{offer, url}`，Host 未启动时报错 |
| `bridge_pair_respond(requestId, allowed)` | 回应待处理配对，返回是否找到该请求 |
| `bridge_devices` / `bridge_revoke_device(deviceId)` | 读取设备表 / 吊销设备 |

事件：`bridge:host-status`（`bool`）、`bridge:pair-request`（`PairingRequest{requestId, deviceId, deviceName, verificationCode}`）。

Host 状态不持久化，应用启动时不会自动开启。用户在设置页点击开启后才会运行，退出应用即停止。

移动端（`client_commands.rs`）：`bridge_connect({offerUrl, deviceName})`、`bridge_resume`（只恢复 `paired=true` 的 profile）、`bridge_disconnect`、`bridge_status`（`BridgeClientStatus`）、`bridge_rpc(method, params)`。事件：`bridge:status`、`bridge:progress`（`relayConnecting` → `e2eeHandshake` → `pairing` / `authenticating` → `connected`）、`bridge:pair-pending`、`bridge:event:*`。这些命令和事件只在移动端编译，与桌面的 `bridge_status` 同名，因此不进 bindings，也被排除在 `events_contract` / `bindings_test` 之外。

移动端的 PDF 读取路径是 `bridge_paper_pdf_info` → 按 256 KiB 循环调用 `bridge_read_bytes`。每个分块都会比对 `size` / `modifiedAt` / `sha256`，不一致就中止。下载完成后按 `path` 存入 IndexedDB `agentero-bridge-files`，按 LRU 淘汰，上限 512 MiB。

## Relay 服务

`server/agentero-relay/relay.mjs`：Node + `ws` 实现的单进程 Paseo v2 兼容 Relay。默认监听 `127.0.0.1:47821`（`HOST` / `PORT` 可覆盖），由 Caddy 终止 TLS 后对外提供 `wss://de.rune.ing:2054/ws`。部署方式见同目录的 `README.md`、`agentero-relay.service` 和 `Caddyfile.example`。

- 握手要求 `v=2`、`serverId` 匹配 `^agt_[A-Za-z0-9_-]{6,}$`、`role` 为 `server` 或 `client`，否则以 1008 关闭。Relay 不做鉴权，`serverId` 只是路由键。
- 新的控制连接会替换旧的。client 接入时若 Host 不在线，以 1013 `host offline` 关闭。Relay 为 client 分配随机 `connectionId` 并通知控制通道。在 Host 数据通道就绪之前，client 帧会被缓冲（上限 64 条或 1 MiB，超出以 1009 关闭）。
- 双向原样转发 text/binary 帧，单帧上限 16 MiB。任一侧断开都会清理这对连接，并向控制通道发送 `disconnected`。控制通道上的 `{"type":"ping"}` 会收到 `pong` 回复。
- 每 30s 发 WebSocket ping，上一轮没有回 pong 的连接被 terminate。`GET /healthz` 返回 `{ok, servers, clients}`。
- `test-relay.mjs` 是本地协议测试。

## 已知限制

- `identity.json` / `devices.json` 先用 `fs::write` 写入，再 chmod `0600`（chmod 仅限 Unix 桌面），写入不是原子的。写入中断留下残缺 JSON 后，`load_or_create`（以及设备表读取）会一直失败，直到手动删除该文件。
- `vault_write_text` 规范化并校验父目录，在该目录内原子替换文件；替换不会跟随末段符号链接。

## 代码

`src-tauri/src/integration/bridge/`、`server/agentero-relay/`、`src/lib/bridge/`、`src/components/settings/panes/remote-access-pane.tsx`、`src/components/mobile/`
