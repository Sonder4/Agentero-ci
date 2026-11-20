# 命令面板 / 快速打开

| 快捷键 | 模式 |
|---|---|
| `⌘P` | 快速打开：论文 + `vault_search` 全文 |
| `⇧⌘P` 或输入 `>` | 执行已注册命令 |

- 居中浮层；`Esc` / `⌘W` / 同键再按关闭（`overlay-stack`）。
- 命中论文 → 打开该 paper；命中路径 → 打开对应文档。
- 论文层（内存）匹配标题 / 作者 / id / 可见标签，空查询显示最近论文。
- 别名层：防抖查 Wiki 索引的 frontmatter `aliases`，命中论文别名时打开该 paper、笔记别名时打开该笔记；与论文层、正文层按路径去重。
- 命令来自进程内注册表（`listCommands()`）：内置命令在加载时显式注册；外部注册位存在，当前没有加载插件，列表为空。重复 id 会被拒绝。外部标题是普通字符串，内置命令仍用 i18n key。快捷键仍只绑定内置 id。
- 实现：`src/components/dialogs/command-palette.tsx`、`src/lib/vault/search.ts`、`src/lib/wiki/api.ts`、`src/lib/shell/commands/`
- Host：`vault_search`（见 [../backend/search.md](../backend/search.md)）、`wiki_search`（见 [../backend/wiki.md](../backend/wiki.md)）

## Navigator

`src/lib/navigator/` 是命令之外的语义层，描述「对哪个对象（target）做哪个动作（action）」，不依赖 DOM。目前有注册表、内置动作工厂、建议协调器和决策网关封装，**还没有 UI 消费方**：命令面板不列 Navigator 动作，建议也不显示。

- **目标**（`types.ts`）：`SemanticTarget` 有 `paper`（vaultId + paperId）、`file`、`pdf-anchor`（页码 + anchor）和 `selection`（panelId + snapshotId）四种，`semanticTargetKey` 生成稳定键。`UIHintTarget`（`ui-control`）只指 DOM 控件，不得序列化成语义目标；F 提示模式不经过 Navigator（见 [shell.md](shell.md)「键盘 Hint 模式与 Vim 键」）。
- **上下文**：`NavigatorContext` 含 target，可选 sourcePanelId / agentSessionId / resourceRevision，另有 capturedAt；`contextFingerprint` 用这几个字段拼接指纹。
- **动作**：`NavigatorAction` 含 `id`、`supports`（类型守卫）、`isAvailable`、`effects`（`read` / `write` / `network` / `cost` / `destructive`）、`confirmation`（`never` / `on-suggestion` / `always`）和 `execute`。
- **注册表**（`registry.ts`）：`registerNavigatorAction` 遇到重复 id 抛错，返回反注册函数。`getNavigatorAction` 先查 Navigator 表，查不到再找命令 registry 里带 `navigatorAction` 字段的命令（`src/lib/shell/commands/types.ts`）。`executeNavigatorAction` 依次检查：未注册返回 `unregistered-action`；不支持该目标返回 `unsupported-target`；调用方传了 `current()` 且目标键或 resourceRevision 已变化，返回 `stale-context`；`isAvailable` 为假返回 `unavailable`。全部通过才执行。错误类型是带 `code` 的 `NavigatorExecutionError`。
- **内置动作**（`actions.ts`）：`createNavigatorActions(handlers)` 生成以下动作，handler 由调用方注入。目前没有地方调用这个工厂，也没有注册这些动作。
  - `paper.open`、`paper.cite`：read，never。
  - `pdf.selection.translate`、`pdf.selection.ask-agent`：read + network，on-suggestion。
  - `pdf.selection.highlight`：write，never。
  - selection 类动作要求目标带 `snapshotId`。
- **建议**（`suggestions.ts`、`suggestion-lifecycle.ts`）：`registerLifecycleHandlers`（`src/lib/lifecycle/register.ts`）启动时调用 `registerSuggestionLifecycle`，订阅 lifecycle 的 `paper:assets-ready` 和 `job:completed`（后者只处理带 `paperId` 的事件），向单例 `suggestionCoordinator` 提交「打开论文」（`paper.open`）建议。
  - 来源固定为 `workflow-rule`，TTL 5 分钟。
  - `intentKey` 由 workflowId、paperId、动作和 resourceRevision（取事件 timestamp）组成。同一 key 已有 pending 或仍在冷却期（默认 30 秒，snooze 默认 10 分钟）时不再提交。
  - 状态从 pending 变为 dismissed / snoozed / expired / accepted；`accept` 只接受未过期的 pending。执行状态由调用方经 `markExecution` 写入。
  - 建议只存在内存里。`reasonKey`（`navigator.assetsReady` / `navigator.jobCompleted`）目前没有对应的 i18n 文案。
- **决策网关**（`decision-gateway.ts`）：`requestPdfSelectionIntent` 封装 Host `decision_gateway_decide`，`decisionId` 固定为 `pdf.selection.intent`。非 Tauri 环境返回 `recommendation: null` 并原样回传允许集合；Host 失败时抛错。返回的只是建议，执行仍走上面的注册表。目前没有调用方。Host 侧规则见 [../backend/decision.md](../backend/decision.md)「前端入口」。
