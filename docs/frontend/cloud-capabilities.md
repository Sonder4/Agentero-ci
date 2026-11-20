# Agentero 云端能力（当前构建已停用）

> **状态**：官方登录与 better-auth Host 插件已移除（`refactor(account): remove official login and better-auth host plugin`）。`src/features/account/visibility.ts` 的 `accountSurfaceEnabled` 固定为 `false`，`VITE_AGENTERO_ACCOUNT_SURFACE` 环境变量不再生效；`release.yml` 中对它的设置是遗留配置。下文保留被关闭的实现说明，供后续决定彻底删除或恢复为构建开关时参考。

当前行为：账户分区、OCR / 额度面板、翻译供应商中的 Agentero 选项和 Embedding 的 Agentero 预设都不渲染；不探测账户会话，不查询额度或恢复 OCR 任务；`src/features/capabilities/data.ts` 的能力请求在发出前抛出 `CAPABILITY_DISABLED`。已有 Agentero 翻译选择在打开设置时回到默认免费引擎；本地能力和第三方 BYOK 配置继续可用。

仍引用该开关的代码分支：`writing-cloud-context.tsx`、`writing-pdf-bar.tsx`、`tree-rows.tsx`、`translate-pane.tsx`、`sync-pane.tsx`、`agent-embedding-block.tsx`、`features/capabilities/data.ts`、`lib/translate/probe.ts`、`lib/translate/services/index.ts`、`use-flush-markdown-on-close.ts`。

## 停用前的实现

以下各节描述开关开启时的行为。

账户设置中的云端区域提供翻译、OCR 和 Embedding 免费剩余额度，以及一个账户级「超额使用 Credit」开关。开关默认关闭，统一控制三项能力；说明通过问号展开，不提供能力消费上限表单。个人和组织账户分别保存设置，组织仅 owner/admin 可修改。

`src/features/capabilities/data.ts` 集中管理会话请求、查询缓存、开关和操作恢复；通过 `tauriFetch` 使用账户会话。原 Rust 账户插件（`src-tauri/src/app/account.rs`，已删除）负责放行 `/api/account/capabilities` 和组织路由，不改变 IPC 命令签名。

## 翻译

选择 Agentero 后，`src/lib/translate/api.ts` 将请求交给已登录的个人账户服务。原文按服务端规范化后的 Unicode 字符计量。网络重试复用持久化幂等键，避免响应丢失造成重复收费；本地只存用户 ID、输入摘要和请求 ID，不保存原文或凭证。其他翻译供应商仍使用已有适配器。

## PDF OCR

在账户设置选择个人或组织账户，上传 PDF 后先报价，再明确开始任务。支持查询恢复、取消和逐页 Markdown 结果。源文件最大 32 MiB、500 页，服务端自动提取已有文本，只对实际成功 OCR 的扫描页计量。此入口不替代本地布局分析和 bounding boxes 工作流。

待恢复记录按用户和账户隔离，保存文件、报价、幂等键和 operation ID。切换账户不复用另一账户的任务。取消停止后续派发；已经发出且成功交付的页可能继续结算。关闭超额开关仅阻止新的付费准入。

## Embedding

Embedding 设置中的 Agentero 预设填入账户 API `/v1` 地址和 `agentero-embedding-v1`，并清空上一供应商密钥。使用 Agentero API Key，由该 Key 绑定的账户支付；并非第三方供应商 BYOK。向量空间固定，不能将旧模型索引直接视为新模型索引。

## 验证

前端变更运行 `pnpm typecheck`、`pnpm deps:check` 和能力数据层测试。原生验证须先完成仓库词典、PDFium 等资源准备，再运行 `pnpm tauri dev`；浏览器页面验证不能替代原生会话联调。

开关仍读取环境变量时，本地开关专项测试覆盖开关未设置、`0`、`1` 三种构建状态，验证翻译供应商、Embedding 预设、额度与 OCR 恢复查询、请求派发及会话探测；新增用例仅用于本地验证，不提交仓库。既有数据层测试验证内部构建的幂等恢复与账户隔离。完整原生资源构建仍需先成功准备 `dictionary/*`，临时去掉词典资源的调试配置仅适合核对设置界面。
