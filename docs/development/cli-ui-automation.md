# CLI 驱动的 UI 自动化（需求与缺口）

> 状态：**部分实现**。`agentero ui actions / click / fill / key / shortcut / command / wait / slide / screenshot`、`ui open / close / window / split` 已实现（见 [../backend/cli.md](../backend/cli.md#触发运行中窗口的控件)）。`ui wait --idle`、`agentero logs`、三平台截图和按原因区分的错误码也已实现。`ui actions --json` 输出控件状态，所有 `ui` 动作返回写入日志的 `actionId`。本文记录需求、已实现内容与仍然存在的限制。总则见根目录 `AGENTS.md`「UI 可脚本化（CLI 驱动）」。

## 目标

每个页面里的按钮和其它控件都能用一条 CLI 指令触发，在 GUI 里产生与点击相同的效果。调试与验收由一串 CLI 指令连续完成：

```text
ui actions → ui click / fill / key → 等待界面稳定 → 截图 → 读日志 → 断言 → 下一步
```

这样调试时不需要 CUA 去模拟鼠标，测试脚本可以重复执行，也可以交给 Agent 运行。

## 分层

| 部分 | 位置 | 语言 | 原因 |
|---|---|---|---|
| CLI 子命令、请求投递、超时、退出码 | `cli/`、`src-tauri` | Rust | 已有实现 |
| 控件发现、点击、填写、按键、状态读取 | `src/lib/shell/commands/desktop-ui-actions.ts` | TS | 只有网页层知道界面上有哪些控件、叫什么、是否可用 |
| 窗口截图 | Host | Rust | 在原生窗口层截图才能拍到 PDF 画布、iframe 与 ChatGPT 子 WebView |
| 日志读取 | CLI | Rust | Host 已把前后端日志写入应用日志目录的 `agentero.log` |

不需要为这套能力更换技术栈。

## 需求

### 已有能力的约束

- 所有可交互控件都要满足 [../frontend/components.md](../frontend/components.md)「可脚本化控件」，能被 `ui actions` 列出。
- 关键流程的控件要有稳定的 `data-ui-action` ID，测试脚本优先用 ID，不用随语言变化的名称。

### 已实现（2026-11-21）

- **等待**：`ui wait <id|名称> [--gone] [--timeout]`，在应用内轮询控件出现或消失，超时以非零状态退出。
- **状态读取**：并入 `ui actions --json`，每个控件带 `role` 与 `state`（勾选、选中、展开、按下、焦点、输入值、滑动条数值与范围）。禁用控件仍不列出，脚本以「不在列表中」判断不可用。
- **滑动条**：`ui slide <id|名称> <值|min|max|+N|-N>`，原生 range 走 input 事件，ARIA slider 走键盘约定。
- **截图**：`ui screenshot [--window] [-o] [--overwrite]`，Host 用 `PrintWindow(PW_RENDERFULLCONTENT)` 截取原生窗口，最小化、隐藏或全黑时报错。
- **动作关联**：本地 `ui` 动作返回 `actionId`，应用日志的 `op start/end ui_action id=…` 使用同一 id；工作区动作记录为 `op end ui_request delivery=…`。

- **空闲等待**：`ui wait --idle` 由 Host 轮询 JobCenter，队列连续 0.75 秒为空才算空闲。
- **日志读取**：`agentero logs path` / `logs tail [--since <actionId>] [--level] [--grep] [-n]`，自动定位各平台日志目录并跨轮转文件读取。
- **其它平台截图**：macOS 用 `screencapture -l` 截原生窗口（需要屏幕录制权限）；Linux 用 WebKitGTK 截主 WebView 的可见页面。
- **错误语义**：前端与 Host 回传 `code`，CLI 原样写入 `error.code`，完整列表见 [../backend/cli.md](../backend/cli.md#触发运行中窗口的控件)。

### 仍然存在的限制

1. `--idle` 只看 JobCenter 任务。Agent 流式回复、PDF 渲染、网络请求不在其中，要用 `ui wait <结果控件>`。空闲车道的后台补齐任务（如大 Vault 的页数统计、双链重建）也算忙，可能让 `--idle` 等到超时。
2. Linux 截图不含标题栏和 ChatGPT 子 WebView；macOS 第一次使用要手动授权，未授权时可能截到空白内容。两者都还没有在真机上验证：macOS 代码由日常 CI 的 macOS 任务编译，Linux 桌面代码只在 Release 构建中编译。
3. 禁用控件仍不出现在 `ui actions` 中，只能通过 `control_disabled` 错误得知。

### Windows 桌面实测（2026-11-20 至 2026-11-21）

在 demo Vault 中用 CLI 验证命令面板、Library 排序、论文及笔记打开和设置窗口。命令面板的可用候选项可以直接点。Library 部分裁剪的行仍可被发现，唯一匹配的列头可先滚入视野再点击。超出虚拟列表渲染范围的控件仍需要先通过应用的搜索或导航定位。

同一条 `ui open` 发出的多路投递在 Host 合并，Windows 深链携带完整 Vault 与论文路径。浏览器报告的 ResizeObserver 循环提示在日志中保留为 WARN。150% 副屏上的截断来自截图坐标的 DPI 虚拟化，按显示器 DPI 重新采集后，主窗口右侧标题栏控件均可见。

### 不在范围内

- 系统文件对话框、系统标题栏：由命令参数直接传路径绕过。
- ChatGPT 子 WebView 与跨域 iframe 内部：由各自宿主接收输入，只通过已注册的 `data-ui-action` 入口与专用子命令操作。
- 视觉细节（动画、残影、像素对齐）：截图后人工检查，必要时用 CUA 抽查。

## 验收

- 用一个 demo Vault（`pnpm demo:vault`）从打开 Vault 到打开论文、高亮、翻译、关闭标签，全程只用 CLI 指令完成，每一步都有截图和对应日志。
- 同一脚本连续运行两次结果一致，不依赖固定的 sleep。
- 切换界面语言后，基于 ID 的脚本仍能运行。
