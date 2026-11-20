# CLI 驱动的 UI 自动化（需求与缺口）

> 状态：**部分实现**。`agentero ui actions / click / fill / key / shortcut / command`、`ui open / close / window / split` 已实现（见 [../backend/cli.md](../backend/cli.md#触发运行中窗口的控件)）。截图、日志读取、等待与状态读取尚未实现，本文记录需求。总则见根目录 `AGENTS.md`「UI 可脚本化（CLI 驱动）」。

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

### 待实现

1. **等待**：`ui wait <id|名称> [--gone] [--timeout]`，等控件出现或消失；`ui wait --idle` 等后台任务队列与正在进行的加载结束。现在 `click` 在动作分派后就返回，对话框与后台任务仍在进行，连续指令之间需要它来避免时序竞争。
2. **状态读取**：`ui get <id|名称> --json` 返回控件的名称、文字、`disabled` / `checked` / `selected` / `expanded`、输入框的值；可选返回当前活动标签与可见面板。
3. **截图**：`ui screenshot [--window <label>] [-o <file>]`，由 Host 截取原生窗口，输出 PNG 路径。窗口被遮挡或最小化时要给出明确错误，而不是返回黑图。
4. **日志**：`agentero logs tail [--since <时间|动作 id>] [--grep] [--level]`，读取应用日志目录中的 `agentero.log`。
5. **动作关联**：每次 `ui` 动作返回一个 id，并写入日志，`logs tail --since <id>` 可以只看这次操作之后的日志。
6. **错误语义**：控件不存在、被禁用、被遮挡、窗口未挂载分别返回不同的 `error.code`，脚本能据此决定重试还是失败。

### 不在范围内

- 系统文件对话框、系统标题栏：由命令参数直接传路径绕过。
- ChatGPT 子 WebView 与跨域 iframe 内部：由各自宿主接收输入，只通过已注册的 `data-ui-action` 入口与专用子命令操作。
- 视觉细节（动画、残影、像素对齐）：截图后人工检查，必要时用 CUA 抽查。

## 验收

- 用一个 demo Vault（`pnpm demo:vault`）从打开 Vault 到打开论文、高亮、翻译、关闭标签，全程只用 CLI 指令完成，每一步都有截图和对应日志。
- 同一脚本连续运行两次结果一致，不依赖固定的 sleep。
- 切换界面语言后，基于 ID 的脚本仍能运行。
