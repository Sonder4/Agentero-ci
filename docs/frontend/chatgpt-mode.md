# ChatGPT Web 侧栏

Agentero 在主窗口右侧栏内嵌一个 ChatGPT 子 WebView，加载 `https://chatgpt.com/`。它与 Agentero 使用同一个持久 WebView2 环境和 profile，账号登录、聊天历史和回复都由 ChatGPT Web 自身管理。Codex 面板继续服务 ACP Agent，ChatGPT 模式使用 ChatGPT 网页端。

## 入口

- 命令面板 `chatgpt.open`、顶部 Agent 按钮右键菜单：在右侧栏打开 ChatGPT。
- 在 PDF、应用内网页和已接入的 Markdown 等正文上划词后，右键选择「在 ChatGPT 中提问」。划词工具栏的更多菜单也有这个入口。
- 在广场的代理网页或 HTML 论文阅读器，点击工具栏 ChatGPT 图标或在正文右键选择「向 ChatGPT 询问此页」，把可读正文、页标题和公开链接加入侧栏。每页最多读取 6000 个 UTF-16 代码单元，超出时显示截短标记。
- 侧栏先显示本地引用，可展开查看文本和出处、逐项移除。点击「附加到 ChatGPT」将选中摘录和出处作为 Markdown 文件上传。当前 ChatGPT 网页输入区会显示可打开和移除的附件卡片，图块布局会预览摘录开头。问题直接在原网页输入框里写，已有文字可继续编辑。
- 一次可逐项附加多条引用。若网页入口不可用，可点「复制引用」。附加操作传输摘录，即使尚未发送消息。用户在 ChatGPT 网页内确认发送。

既有 PDF Ask 卡片的外部聊天交接仍使用对应产品的网页提问入口。引用附件使用 [ChatGPT Web 的文件上传功能](https://help.openai.com/en/articles/8555545-uploading-files-and-audio-to-chatgpt)。

## 信任边界

- 主窗口只创建一个 `chatgpt-sidebar` 子 WebView。切换到 Agent 或收起右栏时，前端保留 WebView 并隐藏它；再次打开 ChatGPT 时恢复原页面与当前对话。主窗口销毁时子 WebView 一并销毁。
- 从文档窗或 Agent / Annotations 功能窗触发的划词动作，会通过 Tauri 窗口事件交回主窗口，再在主窗口右侧栏打开。非主窗口不创建第二份 ChatGPT WebView，也不持有侧栏状态。
- 前端用 `ResizeObserver` 和窗口 resize 事件测量右侧占位区，向 Host 发送 CSS 逻辑坐标。Host 用这些边界创建、定位和缩放子 WebView，页面本身仍然铺满整个侧栏。
- 右侧栏切换使用 `agent` / `chatgpt` 两个 rail surface。Agent 面板与 ChatGPT 子 WebView 的状态都保留在各自 surface 中，切换不会重建 Agent 会话或丢失 ChatGPT 当前页面。
- Tauri capability 以 `webviews` 精确匹配本地 Agentero WebView。`chatgpt-sidebar` 不在匹配列表中，因此远程页面没有 Agentero Tauri IPC、Vault 或文档访问能力。登录提供的 `https` / `about:blank` 弹窗由底层 WebView 原生创建，同样没有 Agentero capability。
- 待附加引用保存在本次主窗口的内存。引用包含用户选中的文本或主动添加的网页摘录。本地文件使用文件名和可用的页码或行号，网页链接去除查询参数和片段。
- 网页正文在用户点击时从当前 iframe 读取，排除隐藏区域、导航、表单和编辑区。Host 只在 `https://chatgpt.com` 当前输入区的通用文件选择器中加入内存生成的 Markdown，网页附件显示完成后才移除本地预览。摘录只通过纯文本写入卡片缩略图，卡片的上传、打开和移除都由 ChatGPT 管理。确认超时时保留引用，重试前可核对网页中的附件。
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

针对 Chrome 的「选项」卡片，扩展静态包显示，后台把所选文字作为独立字段交给侧聊页面。既有网页 REA 证据 `ev_74f65f58d83187d482944779cbf988de421fae0798a17ab78f969dd5c524d0c0` 中，网站检查父框架是否属于官方扩展来源。Agentero 在普通网页模式中通过 Markdown 附件让引用与问题分别出现。

现有网页登录态、会话 Markdown、图片资产、Projects、主题和主页的程序化控制边界见
[ChatGPT Web 能力与桥接边界](../development/chatgpt-web-capability-analysis.md)。桥接层尚未实现，
当前代码没有读取账号、导出会话或管理 Projects 的接口。

## 验证范围

用 [Agentero CLI](../backend/cli.md#触发运行中窗口的控件) 在 Windows WebView2 开发版验证了富文本笔记的全选、右键菜单和添加引用，也验证了文档弹窗将引用交回主窗口侧栏。侧栏重启后继续使用原有登录。PDF 及网页的鼠标划词流程、macOS WKWebView 和 Linux WebKitGTK 尚未完成本次实机验收。

使用 `ui click chatgpt.selection.ask` 和 `ui click chatgpt.quote.attach` 将含中文和数学下标的笔记摘录放入 ChatGPT。已通过截图确认输入区上方的原生附件卡片显示摘录，原有问题草稿保留，通过 Host 日志确认 `chatgpt_sidebar_attach_quote ok=true`。同名文件被 ChatGPT 添加编号时，Agentero 按含摘录哈希的文件名识别附件。

网页正文入口在 Windows WebView2 开发版通过 `ui click chatgpt.page.ask` 将 Cool Papers 和 Papers with Code 添加到同一侧栏。网页右键桥、表单与隐藏元素过滤、截短和来源链接通过本地测试验证。
