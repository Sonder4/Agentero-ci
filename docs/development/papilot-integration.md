# Papilot 排版审阅融合设计

> 状态：**未实现**。对应 [讨论 #24](https://github.com/Sonder4/Agentero-ci/issues/24)。目标是在同一个 Agentero 应用里完成 LaTeX 实时编译、Agent 修改和排版审阅。此前的独立应用交接方案已被取代。个人使用场景下，许可证不作为路线选择条件。

## 源码与归属

下表依据 [Papilot 源码 2793004](https://github.com/PapilotAI/papilot/tree/279300438511ffe2791d3f753a9b66758c280b13) 与 Windows 0.12.0 安装资源。路径以各项目根目录为准。本轮只读源码，未编译或验证行为等价。

| 模块 | Papilot 的做法 | Agentero 的做法 |
| --- | --- | --- |
| 编译与触发 | `src/server/app.ts` 的 manuscript 路由调用 `src/server/manuscript.ts`。后者按工作树快照启动或取消构建、发布状态，失败时保留上次成功 PDF。 `src/web/main.tsx` 与 `compile-button.tsx` 接收状态并提供重编译。 | 复用 `TexProjectView`、`WritingPdfBar`、`ProjectPdf` 与 `src/lib/tex/{auto-build,build-queue}.ts`。将审阅输入的快照与版本校验放到 Rust Host。 |
| 增量与构建诊断 | `engine/src/core/build.ts` 的 `Builder` 调度 latexmk/PhiTeX，复用相近构建缓存，通过 `affectedTargets` 和 `\\includeonly` 缩小构建范围，支持 pass 预览。`compilerDiagnostics` 合并 TeX log 与进程输出；`src/server/build-problems.ts` 归因到文件、行与严重度，并尝试用 SyncTeX 定位。`latex-repair.ts` 为失败构建提出修复建议。 | 保留 `src-tauri/src/features/compile/{mod,runner,diagnostics,project}.rs` 与 JobCenter。先复用 latexmk 增量能力和现有诊断，再按实测迁移缓存、局部编译和额外归因。Papilot 的构建队列、PhiTeX 与自动改配置路径退出。 |
| PDF 显示与排版差异 | `src/web/pdf-view.tsx` 显示稿件。`engine/src/core/publish.ts` 编译两版本，交给 `engine/src/render/{worker,round,locate,synctex,align}.ts` 生成对齐和区域。`{pdf,glyphs}.ts` 用 MuPDF 取字形，`pdfdiff.ts` 比较文本与栅格，`round.ts` 组织审阅单元。 | 选 **EmbedPDF/PDFium** 作为唯一 PDF 渲染与差异输入。`src/lib/pdf/engine.ts` 统一 PDF 资源；`src/components/viewer/pdf/{region-crop,layout-sentence-hover}.ts` 已用区域栅格和文本 run。先做双 PDF 对照，再将可独立的 Papilot 匹配与归因逻辑拆出，在 TS worker 中运行。Papilot 的 PDF.js 视图、MuPDF 渲染进程退出。 |
| 隔离、决策与提交 | `engine/src/core/tasks.ts` 创建 detached worktree，`git.ts` 用临时 index 做快照。`review.ts` 维护接受版本，`delivery.ts` 三方合并并交付，部分模式区分接受与应用。 | 将快照、决策、写回移植到 Rust `features/writing/review`。M1 使用项目范围的磁盘副本，支持非 Git 项目；后续需要合并时可用应用自管 worktree。真实项目的 Git index 不参与任务。保留 `features/writing/sync.rs` 的上传、历史与恢复。 |
| Agent 与提示词 | `src/server/workspace.ts` 排队、组装 prompt、在回合结束后构建和发布。可修的失败可交给 `src/conversation.ts` 的 `buildFixPrompt` 进入新回合。`src/server/writer/{agent,tools,charter}.ts` 定义 `check_build`、`view_pages`、文件编辑和授权。`src/server/{agents,acp,agent-tool-bridge}.ts` 接不同 Agent 与工具。 | 保留 `src/components/agent/agent-panel.tsx` 和 Host `features/agent/{session/run,acp/client,context_turn}.rs`。移植任务约束和修复提示，借鉴 `integration/context_mcp.rs` 的授权机制，提供任务范围的 stdio 工具。Papilot 的 Writer/Pi、Agent 适配层和聊天库退出。 |
| 审阅 UI | `engine/src/web/review.tsx` 展示前后裁剪、轮次与接受、拒绝、评论，`src/web/{review-frame,review-draft-cards}.tsx` 负责嵌入与草稿卡片。 | React 在现有 `TexProjectView` 右区构建 `WritingChangeReview`，复用 PDF、按钮与 i18n。`tex-review-panel.tsx` 继续管理已有的源码评论串，其内容可作为 Agent 上下文。 |
| Fastify API 与存储 | `src/server/{start,app,store}.ts` 提供工作区、稿件、任务、消息、`/api/events` SSE 与应用 SQLite。`engine/src/{client,server}.ts` 管理逐仓库进程及 `publish/view/decide/apply` 路由，`engine/src/core/store.ts` 保存审阅状态。 | 用 typed Tauri IPC、JobCenter、窗口级事件和任务工具接替。去掉 Electron、两层 Fastify、SSE 和第二套数据库镜像。不需要捆绑 Node sidecar。 |
| 设置 | `engine/src/core/config.ts` 检测主文件、引擎、source_dir、latexmkrc 与本机覆盖；`src/web/{settings,latex-settings,model-default-settings}.tsx` 提供配置。 | 复用 `src/lib/tex/config.ts`、写作 manifest、Host 编译配方与 Agent profile。有用的诊断建议移植后仍由作者确认。Papilot 设置库和 `.git/papilot` 不参与运行。 |

## 构建与定位

- **一套正式构建。** 正文、审阅基线、候选都用 Agentero 的 latexmk/recipe runner。普通编辑沿用 CodeMirror 自动保存与约 1 秒防抖，一个项目的运行中构建只留最新尾随请求。基线和候选共享配方选择、工具版本和构建环境，各用独立输出与 aux 缓存。审阅任务的根和配方由 Host 从可信项目解析。前端不能提供任意 scratch 路径或额外命令。M1 审阅双构建使用完整编译，局部编译需要先验证交叉引用、编号和定位。
- **rtex 是独立可选预览模式。** `src-tauri/src/features/rtex/mod.rs` 和 `src/lib/tex-live/session.ts` 已支持 LuaLaTeX 段落画布，但当前画布没有正式 PDF 的双向 SyncTeX。融合流程选正式 PDF 做实时更新与裁决，进入审阅时切到正式模式。原有 rtex 仍可单独开启，与正式预览互斥显示。
- **同代 SyncTeX。** PDF、映射、诊断和日志都绑定输入 revision 和 build ID。新编译失败则保留上一份成功 PDF 及同代映射，失败诊断标注自己的构建。复用 `src/lib/tex/synctex.ts` 与 `src/components/viewer/pdf/hooks/use-pdf-synctex.ts` 的导航。两个审阅产物各查自己的映射，将临时路径转成任务相对路径。当前源码与映射快照一致才跳到可编辑文件。版本不一致时打开只读源码片段，定位不明确时显示原页。

## 用户流程与写回

1. 作者在 `TexProjectView` 左侧编辑 TeX，右侧连续更新 PDF，保留切换源文件、分栏、滚动与缩放行为。从项目、编译问题、TeX 评论或 PDF 区域发起审阅。M1 先支持项目和编译问题。Agent 面板使用原有对话入口，显示所授权的项目与任务。
2. Host 从现有项目准备基线与磁盘副本，保留相对目录和实际依赖。基线与候选之间不能共享可写文件或硬链接。未保存草稿先尝试通过现有编辑器 flusher 保存，失败则取消创建。Vault 的文件仍是正文、笔记和附件的事实来源，`.agentero/catalog.sqlite` 仍管理论文与结构化元数据。临时副本、编译输出和决策只属于审阅任务，不能通过阅读批注、同步或目录扫描意外发布。
3. 审阅 ACP 会话和普通会话分别绑定 scope 与 cwd。工作副本负责隔离文件状态，子进程权限仍由适配器或宿主机制约束。仅启用能落实该权限的本地适配器；如无法限制文件或终端，改用 Host 校验的结构化编辑，并停用不受限制的原生写入工具。不向审阅 Agent 授予裸 Vault、Catalog 或项目外写入能力。
4. Agent 从消息读取诊断，按任务工具在副本中查看和提出修改。Host 在回合结束后编译，把文件、行、严重度、受限日志以及候选结果送回同一会话。`review_check_build` 校验当前副本，`review_read_diff` 返回当前轮次的源码 hunk 与产物信息。M2 的 `review_view_pages` 传有界的 PDF 裁剪与变更区域给支持图片的 Agent。失败时保留副本供手动继续，将用户请求与日志、论文文本分别传入。首版由作者触发下一轮，后续再讨论自动修复次数与费用上限。
5. 审阅区按 hunk 显示 before/after，保留两版本的定位。M1 支持源码片段和同页双 PDF，M2 才做跨页对齐、高亮和裁剪。排版变化由可归因的源码 hunk 带入，无法归因的只是观察结果。若基线本身编译失败，显示基线诊断、源码差异及可用的候选 PDF。旧成功 PDF 可以作为带版本标签的参考，不能冒充基线。
6. 接受、拒绝只记录任务内决定。作者点「应用已接受项」后，Host 重新核对项目、轮次、基线哈希、路径范围和未保存草稿。M1 只应用同一已存在 UTF-8 TeX 文件中互不重叠的 hunks，用现有原子替换能力写回。冲突则保留结果并返回错误。原子替换避免半写，外部编辑器与哈希检查之间仍可能竞争。应用后走现有文件通知、资源失效与重编译路径，不提交、不推送 Git。

### Dockview 与任务状态

源码、正式预览与审阅都留在同一 `TexProjectView`。不新增 Dockview 面板或功能窗，保留 `createViewId()` 与现有项目/路径 params。Dockview 继续负责几何、焦点和布局快照。审阅 ID 不编码进视图或窗口 ID。组件重挂载重新订阅任务快照，退出视图不退订现有窗口级 Agent 事件。

任务有 `taskId`、`roundId`、基线/候选 revision、成对 build ID 和递增状态序号；hunk ID 在所属轮次内有效。Host 会校验窗口、项目和 Vault 的持有关系，迟到构建不能覆盖新轮次。M1 用内存任务表与临时磁盘产物，不迁移 Catalog。取消、切库和退出收回权限与临时资源。后续恢复只设一份审阅状态权威记录，Catalog 如需索引则存可重建投影。

### CLI 可脚本化

每个新增交互控件都要有可访问名称和稳定 `data-ui-action`。下面列出规划入口，具体 DOM、菜单、确认框和输入框均按同一规则验收。

| 操作 | ID | 入口 |
| --- | --- | --- |
| 项目发起、附编译诊断 | `writing.review.start`、`writing.review.attachDiagnostic` | `ui click <id>`，另注册 `ui command writing.review.start` |
| 打开/关闭审阅区、上个/下个 hunk | `writing.review.toggle`、`writing.review.previousHunk`、`writing.review.nextHunk` | `ui click <id>`。列表行使用 `writing.review.select.<hunkId>`，先用 `ui actions --json` 枚举 |
| 接受、拒绝、应用、重试、取消 | `writing.review.accept`、`writing.review.reject`、`writing.review.apply`、`writing.review.retry`、`writing.review.cancel` | `ui click <id>`。确认框的操作分别用 `writing.review.apply.confirm` 与 `writing.review.apply.dismiss` |
| 定位两侧源码 | `writing.review.locateBefore`、`writing.review.locateAfter` | `ui click <id>`，提供画布修饰键手势的等价入口 |
| 评论并重修（M2） | `writing.review.comment`、`writing.review.sendFeedback` | `ui click writing.review.comment` 聚焦编辑，`ui fill <文本>`，再触发发送 |

两个 PDF 查看器的控件要能分别寻址。验收用 `ui wait`、`ui actions --json`、`ui screenshot` 和 `logs tail --since <actionId>`。任务与构建状态可观察，日志记录 task/build ID、状态与错误码，避免记录稿件和 prompt。

## 实施里程碑

### M1：单项目、单文件审阅闭环

一份小型 TeX 项目，保存后自动更新且可双向定位。Agent 获得诊断并在副本提出两个 hunks，双 PDF 同页对照后接受一个、拒绝一个，再显式应用并编译。额外覆盖候选失败、原稿外部改动、未保存草稿和切库。只支持已存在的单一 TeX 文件；不要求作者已有 Git 仓库。

| 文件 | 工作内容 |
| --- | --- |
| 新增 `src-tauri/src/features/writing/review/{mod,commands,snapshot}.rs`，更新 `features/writing/mod.rs` | 任务注册、范围校验、快照、hunk 决策、单文件安全写回、任务取消与清理。 |
| `src-tauri/src/features/compile/{mod,runner,types}.rs`、`src-tauri/src/features/jobs/commands.rs` | 共用受信的构建输入、runner 和队列，用内部审阅构建入口。区分正式/审阅作业与产物目录；已有编译命令签名可沿用。 |
| `src-tauri/src/features/agent/{context_turn,session/run,acp/client}.rs` 与 `integration/context_mcp.rs` 附近 | 现有 Agent 回合增加可选 `reviewTaskId`，由 Host 解析独立 cwd、scope、诊断与工具。普通回合继续走既有入口。 |
| `src/components/writing/{tex-project-view,writing-pdf-bar,writing-problems,writing-desk}.tsx`，新增 `writing-change-review.tsx`、`src/lib/writing/review.ts` | 审阅发起、双 PDF、任务状态和裁决。复用 `text-editor-flush.ts`。给 `use-pdf-synctex.ts` 传入成对产物身份；Agent 面板用既有消息通道接任务卡。 |
| `src/lib/tex/{auto-build,build-state,host}.ts`、`src/i18n/locales/{en,zh-CN}/` | 防抖与 revision 关联、诊断和双语控件文案。 |
| `src-tauri/src/app/{handlers,bindings_test,events_contract}.rs`、`src/lib/core/bindings.ts` | 注册 typed commands 和事件，重新生成并校验 TypeScript bindings。 |

拟增 IPC：

| 命令 | 输入与效果 |
| --- | --- |
| `writing_review_start` | 接收 Vault/项目标识和受信诊断引用，验证窗口与 manifest，创建任务，返回 `taskId`、`baseRevision`。 |
| `writing_review_get` | 用 `taskId` 读授权任务状态、轮次、hunks、诊断和产物句柄。前端订阅再读取快照，用状态序号消除新旧事件竞态。 |
| `writing_review_read_artifact` | 按 `taskId/artifactId` 和区间读取有大小上限的 PDF 二进制。权限绑定窗口、任务和 revision，复用 Host 二进制读取机制，避免整份 PDF 编进 JSON。 |
| `writing_review_decide` | 用 `taskId/roundId/hunkId/decision` 幂等记录接受或拒绝，拒绝旧轮次。 |
| `writing_review_apply` | 按 `taskId/roundId/expectedBaseRevision` 校验编辑器、路径和哈希，应用已接受项。 |
| `writing_review_cancel` | 停止任务的 Agent/编译及其子进程，回收副本；重试复用现有回合或 JobCenter 入口。 |

新增窗口级 `writing:review-changed` 事件，载荷含任务与轮次 ID、状态、递增序号。构建复用 `job:changed` 和 `compile:log`，以 job/build ID 关联。IPC 实现后须再生成、只读校验 bindings，并跑 `app::events_contract`。粗估 20–26 个文件、约 1,300–2,000 行手写改动，不含生成物与一次性夹具。工作量取决于 ACP 权限与构建入口的复用程度。

### M2：可归因的排版差异与审阅反馈

从公式、表格、跨栏和跨页小样本出发，把 Papilot 的归因、对齐和栅格比较移到 EmbedPDF 输入。匹配置信度不足时显示未对齐的两页，不据此作自动裁决。让 Agent 接收 hunk 的版本化证据和有限的页面裁剪。接入评论驱动的下一轮、多文件提案、显式冲突解决，必要时加入内部 worktree 和三方合并。

### M3：性能与桌面三平台验收

用固定配方验证相近构建缓存、局部编译和后台预构建。设置并发、超时取消、PDF worker 内存与缓存额度。测试 Windows Unicode 和长路径、TeX Live/MiKTeX、macOS GUI PATH 与 Linux 依赖和字体。补齐多窗口及崩溃恢复。如局部构建误导编号、引用或 SyncTeX，继续用完整构建。Overleaf、跨设备同步和移动端独立评估。

## 风险与待决定项

- 每次按键都即时重排与保存后自动生成正式 PDF 是两种延迟目标。若要求前者还要双向定位，需额外评估 rtex 位置映射。
- MuPDF 字形轮廓和 PDFium 文本 run 不等价。复杂公式、CJK、合字、浮动体和页码漂移需要样本与低置信回退。
- Agent 原生工具、TeX 配方、符号链接和子进程可突破仅靠副本的隔离。M1 先验证指定 ACP 适配器、配方限制和进程树取消。应用层项目锁也无法替外部编辑器加锁。
- 多文件写回需要整体预检、恢复日志和清晰冲突策略。两轮审阅和 Git 上传不能抢写同一稿件。
- Agentero 已与上游分叉，定时同步停用。记录移植函数的 Papilot 上游提交及对比夹具，控制后续维护面积。

需决定首版的本地 ACP 适配器、是否接受「接受后统一应用」的交互、期望的实时延迟，以及 Windows 先行或桌面三平台同时验收。多文件、自动修复与跨重启恢复是否进入 M2，取决于首版使用结果。
