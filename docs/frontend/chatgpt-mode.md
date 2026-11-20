# ChatGPT Web 侧栏

Agentero 在主窗口右侧栏内嵌一个 ChatGPT 子 WebView，加载 `https://chatgpt.com/`。它与 Agentero 使用同一个持久 WebView2 环境和 profile，账号登录、聊天历史和回复都由 ChatGPT Web 自身管理。Codex 面板继续服务 ACP Agent，ChatGPT 模式使用 ChatGPT 网页端。

## 入口

- 命令面板 `chatgpt.open`、顶部 Agent 按钮右键菜单：在右侧栏打开 ChatGPT。
- 在 PDF、应用内网页和已接入的 Markdown 等正文上划词后，右键选择「在 ChatGPT 中提问」。划词工具栏的更多菜单也有这个入口。
- 在广场的代理网页或 HTML 论文阅读器，点击工具栏 ChatGPT 图标或在正文右键选择「向 ChatGPT 询问此页」，把可读正文、页标题和公开链接加入侧栏。每页最多读取 6000 个 UTF-16 代码单元，超出时显示截短标记。
- 侧栏会显示已选项，可展开查看文本和出处、逐项移除、补充问题。在多页间切换、逐页添加正文或划词，可将多条引用放进同一草稿。
- 「填入 ChatGPT 草稿」把引用和问题填入当前网页对话的空输入框，用户在 ChatGPT 中编辑并确认发送。若网页输入框已有草稿、正在加载或发生上游变更，可点「复制草稿」后手动粘贴。

既有 PDF Ask 卡片的外部聊天交接仍使用对应产品的网页提问入口。划词侧栏草稿留在当前 ChatGPT 对话，通过限定于 `https://chatgpt.com` 的输入操作填入。

## 信任边界

- 主窗口只创建一个 `chatgpt-sidebar` 子 WebView。切换到 Agent 或收起右栏时，前端保留 WebView 并隐藏它；再次打开 ChatGPT 时恢复原页面与当前对话。主窗口销毁时子 WebView 一并销毁。
- 从文档窗或 Agent / Annotations 功能窗触发的划词动作，会通过 Tauri 窗口事件交回主窗口，再在主窗口右侧栏打开。非主窗口不创建第二份 ChatGPT WebView，也不持有侧栏状态。
- 前端用 `ResizeObserver` 和窗口 resize 事件测量右侧占位区，向 Host 发送 CSS 逻辑坐标。Host 用这些边界创建、定位和缩放子 WebView，页面本身仍然铺满整个侧栏。
- 右侧栏切换使用 `agent` / `chatgpt` 两个 rail surface。Agent 面板与 ChatGPT 子 WebView 的状态都保留在各自 surface 中，切换不会重建 Agent 会话或丢失 ChatGPT 当前页面。
- Tauri capability 以 `webviews` 精确匹配本地 Agentero WebView。`chatgpt-sidebar` 不在匹配列表中，因此远程页面没有 Agentero Tauri IPC、Vault 或文档访问能力。登录提供的 `https` / `about:blank` 弹窗由底层 WebView 原生创建，同样没有 Agentero capability。
- 引用和补充问题保存在本次主窗口的内存。引用包含用户选中的文本或主动添加的网页摘录。本地文件使用文件名和可用的页码或行号，网页链接去除查询参数和片段。
- 网页正文在用户点击时从当前 iframe 读取，排除隐藏区域、导航、表单和编辑区。引用草稿通过显式点击填入网页输入框，发送由用户在 ChatGPT 网页内确认。侧栏另有「复制草稿」按钮供手动粘贴。
- Windows 上的 profile 位于 `%LOCALAPPDATA%\<Tauri identifier>\EBWebView`。首次打开 ChatGPT 模式时需要登录，登录 Cookie 随该 profile 持久保存，普通应用更新和进程重启不会清除登录态。Clear browsing data、切换 Tauri identifier 或删除该目录会失去登录态，必须重新登录。它与 Google Chrome 的 Cookie 登录态分开。
- WebView2 代理在首个窗口创建前从 Agentero 当前设置解析。显式代理开启时，主窗口、设置、文档、功能窗、机构登录窗和 ChatGPT 子 WebView 都接收同一份进程级代理快照，因此可以加入同一个 WebView2 环境。显式代理关闭时不向 Tauri 传入代理 URL，由 WebView2 直接遵循 Windows 系统代理、PAC 和 bypass 规则。代理属于环境级配置，修改后需要重启 Agentero 才会应用到 WebView 页面。由于 Tauri/Wry 从 `Url::port()` 构造代理端点，显式 HTTP / SOCKS5 代理必须包含非默认端口。

## ChatGPT 桌面版分析

使用 REA 对本机 Windows 版 ChatGPT `26.1021.3705.0` 做了只读静态分析，安装包 `app.asar` 的 SHA-256 为 `04862A659C6A90FC50514D8C0B8016709D97DECE58839726C57495ED72F1D9F0`，REA Evidence 为 `ev_d334d723022f413f499fd36303bff4341014d38d33e9b1766147dafd8b67a73b`。

已观察到的加载边界：

- Electron 主进程从本地 `webview/index.html` 加载应用前端；打包内容包含 `AppChatgpt`、`ChatGptConversationPage`、消息渲染和会话运行时模块。
- 普通 ChatGPT Chat 是本地 React 前端连接 ChatGPT 账号服务。Codex 是同一应用中的另一条产品路由，历史与服务边界分开。
- 静态分析没有执行打包代码，也没有抓取登录 Cookie 或私有网络请求。该结论只覆盖本次安装包的模块与加载关系，不证明未观察模块的行为。

Agentero 的重建只采用公开可观察的 Web 入口和产品边界。桌面应用内的专有组件、路由、内部接口和资源不进入 Agentero 源码。

## Chrome 扩展参考

OpenAI 的 ChatGPT Chrome 扩展支持在标签页旁聊天，把当前网页或其他打开的标签页加入上下文，选中文字后继续提问，也能引用网页中可用的 YouTube 字幕。侧聊使用 ChatGPT 账号的聊天记录。扩展还可连接 ChatGPT Work 和 Codex，由用户授予网站访问权限后操作网页和标签页。浏览历史有单独的授权入口。[官方使用指南](https://learn.chatgpt.com/docs/chrome-extension) · [Chrome 应用商店](https://chromewebstore.google.com/detail/chatgpt/hehggadaopoacecdllhhajmbjkdcmajg)

本机安装的 `1.26.1112.10914` 扩展采用 Manifest V3。Manifest 将 `background.js` 注册为 service worker，将 `codex-work-sidepanel.html` 注册为侧栏入口，声明 `sidePanel`、`contextMenus`、`scripting`、`tabs`、`nativeMessaging` 和 `debugger`。静态包中可见侧聊的标签页引用脚本 `side-chat-tab-mentions.js`，后台 bundle 中可见右键菜单、选区与 Native Messaging 相关调用。REA 证据 ID 为 `ev_e08d54fbab3dfdcc8d01b61ddcc9e6a837480ebc3ba03809c716544b4c3e3302`；大型 bundle 超出语义分析的内存限额，后台符号由直接读取包文件补充，权限的运行时行为以官方文档为准。

Agentero 实现的范围是阅读提问。网页正文和划词共用 Agentero 的全局 ChatGPT Web 侧栏和引用草稿，跨页提问由读者逐页添加。按标签绑定聊天、枚举 Chrome 标签、YouTube 字幕集成和 Work/Codex 浏览器操作属于扩展的其他功能。

现有网页登录态、会话 Markdown、图片资产、Projects、主题和主页的程序化控制边界见
[ChatGPT Web 能力与桥接边界](../development/chatgpt-web-capability-analysis.md)。桥接层尚未实现，
当前代码没有读取账号、导出会话或管理 Projects 的接口。

## 验证范围

用 [Agentero CLI](../backend/cli.md#触发运行中窗口的控件) 在 Windows WebView2 开发版验证了富文本笔记的全选、右键菜单、添加引用、补充问题和填入网页草稿，也验证了文档弹窗将引用交回主窗口侧栏。WebView 确认草稿已填入，并在输入框已有内容时保留它。侧栏重启后继续使用原有登录。PDF 及网页的鼠标划词流程、macOS WKWebView 和 Linux WebKitGTK 尚未完成本次实机验收。

网页正文入口在 Windows WebView2 开发版通过 `ui click chatgpt.page.ask` 将 Cool Papers 和 Papers with Code 添加到同一侧栏，再通过 `ui actions`、截图和日志确认两条引用、补充问题与原有登录态。既有网页草稿触发了 `composer_occupied`，受保护的草稿得以保留。网页右键桥、表单与隐藏元素过滤、截短和来源链接通过本地测试验证。
