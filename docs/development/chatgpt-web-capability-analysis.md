# ChatGPT Web 能力与桥接边界

**状态**：本文记录早期的可行性分析。已实现的网页操作见 [ChatGPT CLI](../backend/cli.md#管理-chatgpt-网页会话与项目)。

**分析日期**：2026-11-20

**目标**：确认 Agentero 现有 ChatGPT Web 侧栏能否读取登录态、账号、会话、Markdown、图片和 Projects，
并判断主题、项目设置、主页等页面能力能否由代码控制。

本文中的接口和私有请求路径用于记录早期分析。产品中的 ChatGPT CLI 通过现有侧栏的可见 DOM 操作项目、会话和来源。

## 结论

1. **登录信息可以获得，但只应在现有 WebView 内使用。**
   当前 `chatgpt-sidebar` 复用了持久 WebView2 profile。应用重启后仍保持登录。
   页面中的 `/api/auth/session` 和 `/backend-api/me` 可以取得登录状态、账号标识、姓名、邮箱和头像。
   Tauri 2.12.5 也提供 `cookies()` / `cookies_for_url()`，宿主理论上能读取原始 Cookie。
   Agentero 不应把这些原始凭据暴露给插件。插件只接收去敏后的 `signedIn` 与账号摘要。

2. **网页会话 ID 和内容可以获得。**
   DOM 中已经出现 `/c/<uuid>` 与 `/g/<project-id>/c/<uuid>` 链接。
   已登录页面加载当前会话时请求
   `/backend-api/conversations/<conversation-id>?...`，响应包含 `conversation_id`、`messages`、
   `current_node`、消息 id、角色、内容部件和引用 metadata。

3. **文本回复可以按 Markdown 导出。**
   普通文本消息为 `content.content_type = "text"`，正文位于 `content.parts[]` 字符串。
   实测回复中保留了 Markdown 无序列表。链接、标题、表格、代码围栏和 LaTeX 可以直接保留，
   但导出器仍需过滤 system、hidden、tool 和分支消息。

4. **图片可以支持，但需要资产归一化。**
   含图片的消息使用 `content_type = "multimodal_text"`。
   非文本 part 可以是 `image_asset_pointer`，引用
   `file-service://...`、`sediment://...` 或生成图片结果。
   这些值不能直接作为长期 Markdown URL。桥接层需要把资产下载到 Agentero 管理的附件存储，
   再把消息转换为本地图片引用。

5. **Projects 的主要能力已经存在。**
   `GET /backend-api/gizmos/snorlax/sidebar?...` 返回项目 id、instructions、名称、描述、
   主题、记忆范围、文件列表、项目会话和 `can_*` 权限。
   同页 bundle 中已定位到项目创建 `POST /backend-api/projects`、项目删除
   `DELETE /backend-api/gizmos/{gizmo_id}` 和项目会话读取
   `GET /backend-api/gizmos/{gizmo_id}/conversations`。
   项目设置 UI 同时提供重命名、项目设置、项目主页和删除入口。

6. **主题可以控制，主页和会话可以导航。**
   主题读取字段是 `/backend-api/settings/user` 的 `settings.chat_theme`。
   主题写入使用
   `PATCH /backend-api/settings/account_user_setting?feature=chat_theme&value=<theme>`。
   主页、项目页、普通会话和项目会话可以通过
   `/`、`/g/<project-id>`、`/c/<conversation-id>`、
   `/g/<project-id>/c/<conversation-id>` 进行同页导航。

7. **公开 API 不能读取现有 ChatGPT Web 会话。**
   OpenAI 公开的 Conversations API 管理 API 资源中的 conversation。
   Sign in with ChatGPT 的身份 scope 也不会授予现有网页聊天访问权。
   API 平台中的 project 与 ChatGPT Web 的 Project 属于不同资源。

### 能力判定

| 目标 | 判定 | 已确认的机制 | 主要限制 |
|---|---|---|---|
| 判断是否已登录 | 可以 | DOM、`/api/auth/session`、现有持久 profile | 登录页和账号切换事件需持续探测 |
| 获取账号摘要 | 可以 | `/backend-api/me` | 默认只向插件提供 id、显示名、头像 |
| 获取 Cookie / token | 技术可以获得 | WebView2 / Tauri cookie API、页面 session | 禁止交给插件、日志或普通存储 |
| 列出会话和取得 session id | 可以 | `/backend-api/conversations`、`/c/<id>` | 分页、账号和项目范围需校验 |
| 读取会话消息 | 可以 | `/backend-api/conversations/<id>` | 私有 schema，需过滤 hidden/system/tool |
| 导出文本 Markdown | 可以 | `text` + `parts[]` 字符串 | 分支、引用和未知 part 需归一化 |
| 导出图片 | 结构已确认，下载待补验 | `multimodal_text` + `image_asset_pointer` | 需认证解析并落到 Agentero 附件层 |
| 读取 Projects 与内容 | 可以 | sidebar project response、files、conversations | 私有字段随页面版本变化 |
| 创建和删除 Projects | 已通过网页控件验证创建 | 项目创建表单 | 删除未实测 |
| 修改项目设置 | 已通过网页控件验证 | 打开项目设置保存说明，再重新打开核对 | 自动保存写接口尚未完整捕获 |
| 修改主题 | 请求路径已定位 | `PATCH /settings/account_user_setting` | 尚未执行写操作实机验证 |
| 打开主页、项目页、会话页 | 可以 | 产品 URL 导航 | 推荐内容和 UI 状态依赖页面版本 |

## 推荐边界

继续只使用现有的 `chatgpt-sidebar`。不创建第二个 WebView、不复制 profile、不启动独立浏览器，
也不把 ChatGPT Cookie 或 Bearer token 交给插件进程。

在 Host 内增加一个 `ChatGptWebAdapter`，连接同一个 `chatgpt-sidebar`。插件只看到版本化、
高层的 `chatgpt.web.*` 命令和结构化结果。页面内的私有接口、DOM selector、身份头和资产指针
都由 Adapter 封装。

四种实现层的优先级如下：

| 层 | 用途 | 稳定性 | 推荐范围 |
|---|---|---:|---|
| 页面导航 | 打开主页、项目、会话和设置 | 中 | 导航、打开已有对象、降级操作 |
| DOM / accessibility adapter | 操作只存在于 UI 的项目设置等入口 | 中低 | 有确认步骤的写操作和兼容回退 |
| ChatGPT 私有 `backend-api` | 批量读取会话、项目、设置和资产 | 低到中 | 受控 Host Adapter，禁止直接暴露 |
| OpenAI 公开 API | 新建独立 API 会话和稳定模型调用 | 高 | 与 ChatGPT Web 数据分开的产品能力 |

## 实机与 REA 证据

本次使用 REA 6.4.0 连接 Agentero 现有 WebView2 子 WebView。目标是
`https://chatgpt.com`，浏览器为 `Edg/151.0.4129.86`。没有输出 Cookie、Bearer token、
Authorization 头值或原始账号资料。

| Evidence | 证据类型 | 覆盖 |
|---|---|---|
| `ev_8bdcdc6ab163ef9ee99eaf3984f33875715a60b5984463e93a40263bc43e8a88` | CDP target list | 确认 `chatgpt-sidebar` 是现有页面目标 |
| `ev_166d889135c9dad6cf73f25c20bb5f35599371546b754539d41b04c5aab29a5a` | Passive page capture | 已登录 DOM、会话链接、storage key 名、页面资源 |
| `ev_d649a464708ee3178868544789a703ff8f5e54f6ad35f6dc240435def6e836c7` | Conversation page capture | 当前会话 URL、DOM、accessibility text 和持久登录页面状态 |
| `ev_298c0889219e6d8aa5fd86a16216f53d7396ac620e16110b8a6a485383882c70` | Static web bundle analysis | 179 个脚本、8,868,227 个 AST 节点、路由和内容类型字符串 |
| `ev_e7ae3840199d34ae05f2bd5c92226311e68bd3e345e9b78240d188ea7184de00` | Execution observation | 只覆盖到 document reload，精确覆盖率因文档变更不可用 |

`ev_e7ae...` 的 coverage 为 `unavailable`，不能据此推断完整执行链。
私有接口字段、项目 CRUD 和主题 PATCH 还结合了一次只读运行时网络形状捕获和同 bundle
字符串上下文核对。该补充捕获只保留字段名、HTTP method、状态码和内容类型形状。

官方能力边界：

- [Sign in with ChatGPT](https://developers.openai.com/siwc/quickstart)
- [Conversations API](https://developers.openai.com/api/reference/resources/conversations)
- [Conversation items API](https://developers.openai.com/api/reference/python/resources/conversations/subresources/items/methods/list)
- [API RBAC 与 API project](https://developers.openai.com/api/docs/guides/rbac)
- [Responses API](https://developers.openai.com/api/reference/responses/overview)

## 登录态

### 已验证

- 当前子 WebView 打开后直接显示已有历史、Projects、账号头像和套餐信息。
- 本次分析从上一个已退出进程重新启动 Agentero，页面仍是登录状态。
- Windows profile 位于
  `%LOCALAPPDATA%\com.poco-ai.agentero\EBWebView`，应用更新和普通重启沿用该目录。
- 页面上下文读取 `/api/auth/session` 成功。响应字段名包括
  `accessToken`、`account`、`user`、`expires`、`authProvider`。
- `/backend-api/me` 返回账号 id、姓名、邮箱、头像和国家相关字段。
- 会话和设置请求带有 `Authorization`、`ChatGPT-Account-Id`、
  `OAI-Device-Id`、`OAI-Language` 等请求头。这里只记录头名称。

### 关键限制

Cookie 能证明会话存在，但不能直接替代 ChatGPT Web 的认证请求构造。
只依赖 Cookie 的同源 `fetch` 在补充探测中对 settings 和 project 请求返回 401，
对 conversation 请求返回 404。ChatGPT 页面自己的请求包装器会附加授权头和账号头后返回 200。

因此 Adapter 应选择以下一种实现：

- 在页面正文上下文中复用 ChatGPT Web 的认证状态，并向 Host 返回去敏结果。
- 在 Host 内使用底层 WebView2 的受控请求能力，凭据只在内存中短时存在。
- 通过被动捕获页面自己的请求响应读取数据，并显式发起由用户触发的 UI 操作。

禁止把 Cookie、access token 或完整身份头写入插件配置、日志、Vault 或普通缓存。

## 会话与 Markdown

### 数据结构

实测 endpoint：

```text
GET /backend-api/conversations/<conversation-id>?num_turns=10&include_has_versions=true
GET /backend-api/conversations?exclude_conversation_origin=tpp&expand=false&
    hide_snorlax=false&is_archived=false&is_starred=false&limit=20&order=updated&offset=0
```

conversation 响应顶层包含：

```text
title
conversation_id
gizmo_id
create_time
update_time
current_node
messages[]
```

message 的关键字段：

```text
id
author.role
create_time
content.content_type
content.parts[]
metadata.content_references[]
metadata.citations[]
metadata.is_visually_hidden_from_conversation
```

当前测试会话只含普通文本。实测统计为 4 条消息，角色是 system、user、assistant，
4 个内容类型均为 `text`，4 个 part 均为 string。assistant 文本包含 Markdown 无序列表。

### Markdown 映射

正文字符串应保留原始 Markdown。转换器只处理消息边界和附件，不重建标题、列表、表格、
代码围栏、HTML、LaTeX 或引用。

建议的内部模型：

```ts
type ChatGptMessage = {
  id: string;
  role: "user" | "assistant" | "tool" | "system" | "unknown";
  createdAt: number | null;
  blocks: ChatGptContentBlock[];
  hidden: boolean;
};

type ChatGptContentBlock =
  | { type: "markdown"; text: string }
  | { type: "image"; asset: AgenteroAssetRef; alt: string }
  | { type: "file"; asset: AgenteroAssetRef; name: string }
  | { type: "citation"; title?: string; url?: string }
  | { type: "unknown"; contentType: string };
```

导出规则：

- 默认隐藏 `system`、tool、visually hidden 和认证内部消息。
- 有分支时按 `current_node` 选择活动路径。响应同时提供 `mapping` 时必须使用它重建路径。
- 不把 `metadata.citations` 拼入正文原文，单独生成引用列表。
- 未知 content type 作为 `unknown` block 保留，不能静默丢弃。
- 流式回复完成后再生成稳定 Markdown。进行中的消息标记为 transient，不写入持久缓存。

## 图片与文件

静态 bundle 已确认以下内容协议：

- text-only 消息：`content_type: "text"`
- 含附件消息：`content_type: "multimodal_text"`
- 图片 part：`content_type: "image_asset_pointer"`
- 常见 asset pointer：`file-service://...`、`sediment://...`
- 生图结果还可能包含 `data:image/...`、base64 PNG 或 GIF spritesheet metadata
- 相关 endpoint 包括 `/backend-api/estuary/content` 和
  `/backend-api/estuary/public_content/enc`

图片不能直接转换成可长期访问的网页 URL。Adapter 应：

1. 读取 `asset_pointer`、width、height、mime type 和名称。
2. 在已认证页面或 Host 内解析为字节。
3. 写入 Agentero 按引用计数的附件存储。
4. 在 Markdown 中使用 Agentero 自己的 `asset://` /附件引用，并保留 alt 文本。
5. 对 GIF 保留原图，不默认取静态 spritesheet。

本次在 Windows 上确认了图片协议和引用类型，没有用一个含图片的临时会话完成下载到附件层的
端到端验证。图片加载应作为独立能力探测，失败时保留原 message 和 asset pointer。

## Projects

### 已验证的读取字段

Projects sidebar 响应按项目返回：

```text
items[].gizmo.gizmo.id
items[].gizmo.gizmo.instructions
items[].gizmo.gizmo.display.name
items[].gizmo.gizmo.display.description
items[].gizmo.gizmo.display.emoji
items[].gizmo.gizmo.display.theme
items[].gizmo.gizmo.memory_enabled
items[].gizmo.gizmo.memory_scope
items[].gizmo.gizmo.current_user_permission.can_*
items[].gizmo.files[]
items[].conversations.items[]
```

文件项包含 `file_id`、`name`、`type`、`location`、`created_at`、`library_file_id`
和 knowledge-store metadata。项目会话项包含 conversation id、标题、时间、gizmo id、
归档/关注/临时会话状态和 owner。

### 已定位的写路径

同一页面 bundle 中出现：

```text
POST   /backend-api/projects
PATCH  /backend-api/settings/account_user_setting?feature=chat_theme&value=<theme>
DELETE /backend-api/gizmos/<gizmo-id>
GET    /backend-api/gizmos/<gizmo-id>/conversations
```

这些 method 和 route 来自同页 bundle 的静态上下文及项目设置 UI 对照，能够作为 Adapter 的实现候选。
本次没有调用项目创建、删除或主题 PATCH，因此它们不等于已经通过真实写入测试。

项目创建请求体字段为：

```text
name
instructions
emoji
memory_scope
theme
```

UI accessibility tree 还确认项目菜单包含：

- 重命名项目
- 项目设置
- 项目主页
- 置顶项目
- 删除项目

项目设置面板包含项目名称、说明/instructions、记忆范围、资料库访问权限、图标颜色和删除项目。
项目设置的自动保存请求路由尚未在这次捕获中完整出现，因此 v1 应通过同一 WebView 的 DOM/UI
适配完成编辑，或在实现前补一次带备份与回滚的写请求验证。

## 主页、主题与设置

| 表面 | 读取 | 控制 | 稳定性 |
|---|---|---|---|
| 主页 | `/backend-api/tpp/default-tab-recommendation` 等首屏数据 | 导航到 `/`，UI 选择推荐项 | 导航中，推荐 feed 低 |
| 普通会话 | `/backend-api/conversations/<id>` | 导航 `/c/<id>` | 中 |
| 项目页 | project response | 导航 `/g/<project-id>` | 中 |
| 项目会话 | project conversations response | 导航 `/g/<project-id>/c/<id>` | 中 |
| 主题 | `settings.chat_theme` | PATCH `settings/account_user_setting` | 低到中 |
| 设置 | `/backend-api/settings/user` | 逐 feature 的 PATCH endpoint | 低 |

主页和设置页内部大量依赖 feature flag、announcement 和 A/B payload。
Adapter 只暴露产品级操作，不把通用 HTTP 或任意页面脚本提供给插件。

## 推荐 Adapter API

```ts
interface ChatGptWebCapability {
  session: {
    getStatus(): Promise<{
      signedIn: boolean;
      accountId?: string;
      displayName?: string;
      avatarUrl?: string;
    }>;
  };

  conversations: {
    list(options?: ChatGptConversationListOptions): Promise<ChatGptConversationSummary[]>;
    get(id: string): Promise<ChatGptConversation>;
    getMarkdown(id: string, options?: { includeImages?: boolean }): Promise<ChatGptMarkdownDocument>;
    open(id: string): Promise<void>;
    send(input: ChatGptSendInput): Promise<ChatGptSendResult>;
  };

  projects: {
    list(): Promise<ChatGptProjectSummary[]>;
    get(id: string): Promise<ChatGptProject>;
    create(input: ChatGptProjectCreateInput): Promise<ChatGptProject>;
    open(id: string): Promise<void>;
    delete(id: string): Promise<void>;
  };

  settings: {
    getTheme(): Promise<ChatGptTheme>;
    setTheme(theme: ChatGptTheme): Promise<void>;
  };

  navigate(target: ChatGptNavigationTarget): Promise<void>;
}
```

插件权限建议：

```text
chatgpt.profile.read
chatgpt.conversations.read
chatgpt.conversations.write
chatgpt.projects.read
chatgpt.projects.write
chatgpt.settings.write
chatgpt.assets.read
```

账号 email、原始 token、Cookie、完整请求头和 profile 目录不属于插件 API。

## 同页桥接设计

桥接必须附着于 `chatgpt-sidebar` 本身：

1. Host 维护一个 `ChatGptWebAdapter` 单例，只有它在内存中处理认证上下文。
2. 页面桥只接受固定 action 列表，例如 `conversation.get`、`project.list`、
   `settings.setTheme`。拒绝任意 JavaScript 和任意 URL。
3. 每个请求使用 UUID、超时、取消和最大响应体积。页面返回结构化 JSON，不返回 Cookie 或头值。
4. Host 校验当前 origin、主 frame、账号身份和请求序号，账号切换后取消未完成请求。
5. 响应先进入版本化 schema 校验，再转换成 Agentero domain model。
6. 图片和文件由单独资产通道处理，插件只能拿到 Agentero asset handle。
7. 页面隐藏时保留同一 WebView 和当前页面状态。主窗口销毁时统一销毁。
8. WebView2 可在 Windows 上使用底层 `ICoreWebView2` 的 script/message channel。
   macOS 与 Linux 需要单独的 WKWebView / WebKitGTK adapter，先保持接口一致。

当前 Tauri 2.12.5 的 `Webview::eval` 是单向执行入口。需要返回值的桥接不能只调用 `eval`，
应增加受控的 WebView message channel。Windows 可以直接使用项目已依赖的 `webview2-com`。

本次 REA 使用的临时 CDP 端口只用于开发取证。发布构建不得开放通用远程调试端口，也不得向插件
界面暴露页面执行器。

## 版本与降级

Adapter 必须带运行时能力探测：

```text
session endpoint available
conversation schema version recognized
message content types recognized
project sidebar schema recognized
theme setting path available
asset resolver available
```

每项探测失败只关闭对应能力。会话面板仍可手动使用，不能因为导出失败阻断正常 ChatGPT Web。

私有 API 或 DOM 改版时：

- 返回稳定错误码 `chatgpt_web_capability_unavailable`。
- 保留最后成功响应，但标记 stale，不继续模拟新数据。
- 对写操作在请求结果未知时进入 `unknown`，先刷新页面确认状态，再提示重试。
- 不在插件中硬编码 endpoint、selector 或 WebView2 实现细节。

## 实施顺序

1. **只读会话快照**：账号摘要、conversation id、消息角色、Markdown text parts。
2. **Markdown 导出**：过滤 hidden/system 消息、分支路径、citation 外置。
3. **附件通道**：图片、文件、GIF 下载到 Agentero 资产层。
4. **Projects 只读**：项目列表、instructions、文件 metadata、项目会话。
5. **Projects 安全写操作**：创建、删除、打开。所有破坏性操作要求显式确认。
6. **项目设置和主题**：先 UI adapter，补请求证据后再切私有写接口。
7. **插件 capability**：把 Adapter 投影为受权限控制的插件服务和事件。

## 未解决项

- 没有用含图片的临时会话完成图片下载、缓存和 Markdown 渲染的端到端测试。
- 没有执行项目创建后立即删除的破坏性实机验证。
- 项目设置自动保存的完整写请求未捕获。
- 会话发送的 sentinel、proof token、SSE/WebSocket 流协议和失败恢复尚未验证。
- macOS WKWebView 与 Linux WebKitGTK 的页面消息通道仍需单独实现和实机验证。
- ChatGPT Web 私有接口没有公开兼容性契约，必须作为可替换 Adapter 维护。
