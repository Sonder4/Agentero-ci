# ChatGPT Web 侧栏

Agentero 在主窗口右侧栏内嵌一个 ChatGPT 子 WebView，加载 `https://chatgpt.com/`。它与 Agentero 使用同一个持久 WebView2 环境和 profile，账号登录、聊天历史和回复都由 ChatGPT Web 自身管理。Codex 面板继续服务 ACP Agent，ChatGPT 模式使用 ChatGPT 网页端。

## 入口

- 命令面板 `chatgpt.open`、顶部 Agent 按钮右键菜单：在右侧栏打开 ChatGPT。
- 任意已接入的正文选区菜单和 PDF 选区菜单：`在 ChatGPT 中提问`。Agentero 冻结选区、来源标题、URL、页码或行号，生成提问草稿并交给 ChatGPT Web。
- PDF Ask 卡片：沿用同一个侧栏和交接逻辑。

Agentero 先尝试把草稿复制到系统剪贴板，再让右侧栏子 WebView 导航到 ChatGPT Web 的 `?q=` 入口。当前 Windows 实机测试中，复用的 ChatGPT 会话会把 `q` 中的提示词填入输入框，用户点击发送后 ChatGPT 能读取选区并回复。`?q=` 是网页入口行为，不是稳定公开 API；部分入口或上游版本可能直接发送，如果只填入草稿，剪贴板中的同一内容仍可手动粘贴。

## 信任边界

- 主窗口只创建一个 `chatgpt-sidebar` 子 WebView。切换到 Agent 或收起右栏时，前端保留 WebView 并隐藏它；再次打开 ChatGPT 时恢复原页面与当前对话。主窗口销毁时子 WebView 一并销毁。
- 从文档窗或 Agent / Annotations 功能窗触发的划词动作，会通过 Tauri 窗口事件交回主窗口，再在主窗口右侧栏打开。非主窗口不创建第二份 ChatGPT WebView，也不持有侧栏状态。
- 前端用 `ResizeObserver` 和窗口 resize 事件测量右侧占位区，向 Host 发送 CSS 逻辑坐标。Host 用这些边界创建、定位和缩放子 WebView，页面本身仍然铺满整个侧栏。
- 右侧栏切换使用 `agent` / `chatgpt` 两个 rail surface。Agent 面板与 ChatGPT 子 WebView 的状态都保留在各自 surface 中，切换不会重建 Agent 会话或丢失 ChatGPT 当前页面。
- Tauri capability 以 `webviews` 精确匹配本地 Agentero WebView。`chatgpt-sidebar` 不在匹配列表中，因此远程页面没有 Agentero Tauri IPC、Vault 或文档访问能力。登录提供的 `https` / `about:blank` 弹窗由底层 WebView 原生创建，同样没有 Agentero capability。
- Agentero 只在用户主动点击划词动作或命令时传递选区文本和来源标题、去除 query/hash 的来源 URL、页码或行号。选区的相邻上下文、未选中的 Vault 内容和其他文档内容不会自动加入提示词。
- `?q=` 会把提示词放进 WebView 导航 URL，可能进入该 WebView 的本机历史。Agentero 也会尝试把同一提示词写入系统剪贴板作为手动粘贴回退，因此两项都应视为本机可见数据。
- Windows 上的 profile 位于 `%LOCALAPPDATA%\<Tauri identifier>\EBWebView`。首次打开 ChatGPT 模式时需要登录，登录 Cookie 随该 profile 持久保存，普通应用更新和进程重启不会清除登录态。Clear browsing data、切换 Tauri identifier 或删除该目录会失去登录态，必须重新登录。它与 Google Chrome 的 Cookie 登录态分开。
- WebView2 代理在首个窗口创建前从 Agentero 当前设置解析。显式代理开启时，主窗口、设置、文档、功能窗、机构登录窗和 ChatGPT 子 WebView 都接收同一份进程级代理快照，因此可以加入同一个 WebView2 环境。显式代理关闭时不向 Tauri 传入代理 URL，由 WebView2 直接遵循 Windows 系统代理、PAC 和 bypass 规则。代理属于环境级配置，修改后需要重启 Agentero 才会应用到 WebView 页面。由于 Tauri/Wry 从 `Url::port()` 构造代理端点，显式 HTTP / SOCKS5 代理必须包含非默认端口。

## ChatGPT 桌面版分析

使用 REA 对本机 Windows 版 ChatGPT `26.1021.3705.0` 做了只读静态分析，安装包 `app.asar` 的 SHA-256 为 `04862A659C6A90FC50514D8C0B8016709D97DECE58839726C57495ED72F1D9F0`，REA Evidence 为 `ev_d334d723022f413f499fd36303bff4341014d38d33e9b1766147dafd8b67a73b`。

已观察到的加载边界：

- Electron 主进程从本地 `webview/index.html` 加载应用前端；打包内容包含 `AppChatgpt`、`ChatGptConversationPage`、消息渲染和会话运行时模块。
- 普通 ChatGPT Chat 是本地 React 前端连接 ChatGPT 账号服务。Codex 是同一应用中的另一条产品路由，历史与服务边界分开。
- 静态分析没有执行打包代码，也没有抓取登录 Cookie 或私有网络请求。该结论只覆盖本次安装包的模块与加载关系，不证明未观察模块的行为。

Agentero 的重建只采用公开可观察的 Web 入口和产品边界。桌面应用内的专有组件、路由、内部接口和资源不进入 Agentero 源码。

## 当前范围

当前提供主窗口右侧栏内的 ChatGPT Web 模式和划词提交。后续若要求 Agentero 原生渲染聊天与流式消息，应另行接入官方支持的公开认证与 Responses 接口，并避免依赖从桌面安装包提取的私有协议。

现有网页登录态、会话 Markdown、图片资产、Projects、主题和主页的程序化控制边界见
[ChatGPT Web 能力与桥接边界](../development/chatgpt-web-capability-analysis.md)。桥接层尚未实现，
当前代码没有读取账号、导出会话或管理 Projects 的接口。

## 验证范围

已在 Windows 实机完成开发版验证：通过命令面板和 Agent 菜单打开侧栏、在 Agent / ChatGPT 之间切换并恢复原对话、拖动 Dockview 右栏改变子 WebView 尺寸、从主窗口和独立文档窗分别划取测试文本并在侧栏获得回复、重启应用后继续使用同一登录状态和历史。验证只覆盖 Windows WebView2；macOS WKWebView 与 Linux WebKitGTK 尚未实机验证。
