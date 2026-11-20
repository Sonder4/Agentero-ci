# 广场 Plaza

外部发现入口：嵌入的论文站点和原生面板（订阅、arXiv 每日推荐、CCF 截稿、Skill 推荐）。全部是虚拟路径 `agentero:plaza/<id>`，不写盘；入库才进 Vault。

## 打开与标签

- 文件树根上的 `agentero:plaza` 是纯虚拟文件夹，单击只展开 / 收起，没有广场首页（树行见 [vault-tree.md](vault-tree.md)）。子行由 `visiblePlazaSources(plazaHiddenSources)` 生成；右键父节点逐条勾选显隐。
- 点子行 → `openPlazaSource` → `setTreeSelectedPath` + `openTab(source.path)`。广场路径属于稳定虚拟路径，标签 id 就是路径本身（不是 `view-` UUID），再次打开只聚焦。
- `loadTabResources` 对广场路径直接返回 `kind: "plaza"`，不探测文件系统。`doc-view.tsx` 渲染懒加载的 `PlazaTabView`：首次激活才挂载 `PlazaView`（恢复出的后台标签不发请求），之后保留到标签关闭。
- 标签右键 **移动至新窗口** 对广场开放（Library / Trash / Zen 不行），流程同 [workspace.md](workspace.md)。**复制路径**和论文路径条不显示。
- `plazaEnabled` 关闭：树里不渲染广场节点，`openPlazaSource` 不响应，`closePlazaTabs()` 关掉所有广场标签，`doc-view` 也不挂载面板（含站点 iframe 和订阅轮询）。

## 来源

注册表 `PLAZA_SOURCES`（`src/lib/plaza/sources.ts`）。新增来源只加一条，树行和面板都从这里派生；图标名到组件的映射在 `components/plaza/source-icons.ts`，lib 层不引组件。

| id | 形态 | 内容 |
|---|---|---|
| `cool-papers` | iframe，`agentero-coolpapers` | papers.cool |
| `modelscope` | iframe，`agentero-modelscope`，`desktopWidth: 1280` | ModelScope Papers |
| `papers-with-code` | iframe，`agentero-pwc` | paperswithcode.co 会议页 |
| `skills` | 原生 `PlazaSkillsView` | Skill 推荐 |
| `feeds` | 原生 `PlazaFeedsView` | 订阅 |
| `arxiv-rec` | 原生 `PlazaArxivRecView` | arXiv 每日推荐 |
| `ccf-deadlines` | 原生 `PlazaCcfDeadlinesView` | CCF 截稿 |

## 站点 iframe（`PlazaWebFrame`）

每个站点走自己的 Host scheme（`app/mod.rs` 注册，`src-tauri/src/features/paper/discovery/proxy/`、`coolpapers/proxy.rs`），origin 写死，不是开放中继；和网页论文的任意 host 代理 `agentero-web` 分开（[web-view.md](web-view.md)、[../backend/api.md](../backend/api.md) §3.6.2）。Windows 上 origin 是 `http://<scheme>.localhost`，其它平台是 `<scheme>://localhost`。代理重建 HTML 时剥掉上游 X-Frame-Options / CSP，注入广场导航脚本和共享选区桥 `inject_selection_bridge`。工具栏的「向 ChatGPT 询问此页」与正文右键动作见 [ChatGPT Web 侧栏](chatgpt-mode.md)。

- sandbox `allow-scripts allow-same-origin allow-forms`，不给 `allow-popups`：`target="_blank"` 由代理改成原地导航。
- 消息 `source: "agentero-plaza"`，校验 `event.origin === embedOrigin` 且 `event.source` 是本 iframe（保留的其它标签可能有同源 frame）：

| 字段 | 处理 |
|---|---|
| `path` | 压入自建历史栈（截掉前进分支） |
| `external` / `externalPath` | 第三方链接 / 站点拒绝内嵌的路径 → `openExternalUrl` |
| `importPaper` | 行内「入库」→ 弹文件夹选择 |
| `queryImported` | `importedIdsInLibrary` 回答哪些行已在库（≤200 个；按去版本号的 arXiv id 或 `pwc-<id>` 文件夹匹配） |
| `panX` / `zoomRatio` | frame 吃不掉的横向滚动 / Ctrl·⌘ 滚轮或捏合缩放 |

回帖 `source: "agentero-plaza-host"`（`importedId` + `ok`，或 `importedIds`）。

- **历史**：后退 / 前进 / 重新加载只改 `epoch` 重挂 iframe；frame 内点击只记栈不重挂。当前路径存 `agentero:plaza:<id>:last-path`，栈存 `…:history`（≤500 项，非同源路径整份丢弃）。
- **缩放**（`use-plaza-frame-zoom.ts`）：缩放 iframe 元素本身，不进跨域文档、不重载。默认按 `min(1, 宽 / desktopWidth)` 自动适配；档位 25%–200%；百分比按钮回到自动适配。
- 划词：`useWebViewSelection` 接同一套桥协议，工具栏 / 翻译 / 快速对话 / 加入对话与网页论文一致（[selection.md](selection.md)）。拖拽期间 `useIframeDragShield` 暂停 iframe 命中测试。
- 工具栏另有「在系统浏览器打开」当前路径。
- **入库**：`MoveDestinationPicker` 选目录 → `importPlazaPaper(request, parentDir)`。`paperswithcode.co/paper/<id>` → 任务 `mode: "plaza", source: "pwc"` → `paper_pwc_import`；arXiv 行走魔棒 `lookupSubmit`（带 arXiv id 和 LaTeX 源码）；其它行 → 任务 → `paper_coolpapers_import`（读该行 papers.cool 页的 `citation_*`）。完成后刷新树和论文库，不打开论文；无 PDF 时 warning toast。魔棒路径 120s 未回调就把该行恢复成可重试。细节见 [paper-import.md](paper-import.md)「广场」、[../backend/academic-search-apis.md](../backend/academic-search-apis.md) §2.9 / §2.12。

## 订阅（Feeds）

Host 存 XDG `feeds.sqlite`，不写 catalog / Vault；命令和过滤语义见 [../backend/api.md](../backend/api.md) §3.10.4，解析见 [../backend/academic-search-apis.md](../backend/academic-search-apis.md) §2.8。前端封装 `src/lib/plaza/feeds.ts`。

- 挂载时 `feeds_list` + `feeds_items`；随即并每 15 分钟 `feeds_refresh({ staleOnly: true })`（静默失败）。刷新按钮只刷选中订阅，未选则全部。
- 添加：输入 URL 或点 arXiv 分类 chip（`cs.AI / cs.CL / cs.LG / cs.CV / stat.ML` → `rss.arxiv.org/rss/<cat>`）→ `feeds_add`。订阅菜单：改名、复制 URL、单独刷新、置顶（`feeds_set_pinned`）、删除。
- 时间线用 `@tanstack/react-virtual`。「隐藏已读」把请求过滤切成 `unread`。
- 打开详情自动 `feeds_set_read`；无 `bodyMarkdown` 时 `feeds_resolve_body` 抓全文。正文用 `MessageResponse` 渲染，外包 `SelectionSurface`（`surface: "feed"`，选中即复制）。
- 卡片右键「加入对话」/ 详情按钮 / 拖进 Agent 输入框（MIME `application/x-agentero-plaza-feed`）都产生同一个虚拟 mention `agentero:plaza/feeds/<id>`，发送时展开标题、来源、URL 和 ≤600 字摘要，不当成可读文件（[agent.md](agent.md)）。点 chip 经 `openPlazaMentionPath` 打开 Feeds 并直接进该条详情。
- 入库：`importFeedItem` 优先 `paperUrl`、否则 `url`，走 `lookupSubmit`；成功或已在库 → `feeds_mark_imported`。
- Host 错误 `feeds.<code>` 由 `hostErrorKey` 映射为 i18n 文案后 `notifyError`。

## arXiv 每日推荐

用 Vault 论文摘要做语料，对当天 arXiv 新论文按 embedding 相似度排序；Host 语义见 [../backend/api.md](../backend/api.md) §3.10.5，前端封装 `src/lib/recommend/`。

- 先 `probe_embedding` 探测设置里的 embedding 端点。探测通过前不显示缓存结果；未配置 / 探测失败给出「打开设置」（Agent 页）。设置里 embedding 变化会清空列表并重新探测。
- 探测通过后 `recommend_arxiv_last` 显示当天存量；刷新或增删分类 → `recommend_arxiv({ force: true })`。分类存 `plaza:arxiv-rec:categories`，默认同上面 5 个 chip，可从全量分类表搜索添加，至少留 1 个。
- 打开本地 Vault 时 `lifecycle/register.ts` 预热一次 `recommend_arxiv`（Host 当天已有结果即短路）。
- 空态：`recommend.no_embedding` / `recommend.probe_failed` / `recommend.empty_corpus` / `recommend.no_candidates`。
- 每条：阅读（`openRemoteArxivPaper`，远程 PDF 预览，不落盘）、打开原文、入库（`lookupSubmit`）。工具栏可整批翻译摘要（`runTranslate`，`surface: "arxiv-rec"`）。摘要外包 `SelectionSurface`（`surface: "abstract"`）。

## CCF 截稿

前端直接 `fetch` 上游 [ccf-deadlines](https://github.com/ccfddl/ccf-deadlines) 的 `https://ccfddl.com/conference/initial.json`，不经 Host，也不自己维护会议数据（`src/lib/plaza/ccf-deadlines.ts`）。

- 每次打开都拉网络；`localStorage` 的 `plaza:ccf-deadlines:v2` 只在离线时兜底（滤掉已过期项）。
- 按 `archive` 字段再拉历史分片，为下一届未公布的会议按上届日期估算截稿：上届在 ~18 个月内、估算落在未来 12 个月内才算，标「估算」，不覆盖官方日期。
- 列表按截稿升序；筛选：CCF 等级 A / B / C / N 多选、CCF 领域单选。每行：等级、截稿倒计时、官网链接。

## Skill 推荐

`SKILL_THEMES`（`src/lib/plaza/skill-catalog.ts`）按科研阶段分组：reading / writing / figures / ppt / reproduce / submit。星数是静态快照，只作展示。点卡片 → `importPlazaSkillRepo(url)` → 魔棒 `lookupSubmit` → `lookup_import_batch` 返回候选 → 多选对话框 → `skill_install`；角上链接开 GitHub。流程见 [../backend/skill-import.md](../backend/skill-import.md) §4。

## 设置与状态

| 项 | 位置 | 作用 |
|---|---|---|
| `plazaEnabled` | 设置（默认开） | 总开关，见上 |
| `plazaHiddenSources` | 设置（默认空） | 隐藏的来源 id |
| `embedding` | 设置 | arXiv 推荐的 embedding 端点 |
| `networkProxy*` | 设置 | Host 侧站点代理、订阅、推荐请求 |
| `agentero:plaza:<id>:last-path` / `:history` | localStorage | 站点 iframe 位置与历史 |
| `plaza:arxiv-rec:categories` | localStorage | 推荐分类 |
| `plaza:ccf-deadlines:v2` | localStorage | CCF 离线兜底 |

设置字段见 [../backend/settings.md](../backend/settings.md)。广场没有专用 zustand store：列表和详情是组件状态，订阅数据以 Host `feeds.sqlite` 为准，已入库判定读 `libraryStore`。

## 代码

| 路径 | 职责 |
|---|---|
| `src/lib/plaza/sources.ts` | 来源注册表、虚拟路径谓词、标题 |
| `src/lib/plaza/import.ts` | 站点行入库路由、`runPlazaImportJob`、Skill 导入 |
| `src/lib/plaza/imported.ts` | 已入库 id 匹配 |
| `src/lib/plaza/feeds.ts` | Feeds IPC、入库、摘要清洗 |
| `src/lib/plaza/feed-mention.ts` / `feed-chat.ts` / `feed-drag.ts` / `mention-open.ts` | 订阅条目 → Agent mention / 拖拽 / chip 打开 |
| `src/lib/plaza/ccf-deadlines.ts` / `skill-catalog.ts` | CCF 数据与估算 / Skill 清单 |
| `src/components/plaza/plaza-tab-view.tsx` / `plaza-view.tsx` | 懒挂载外壳 / 按来源分发 |
| `src/components/plaza/plaza-web-frame.tsx` / `use-plaza-frame-zoom.ts` | 站点 iframe、历史、入库弹层 / 缩放 |
| `src/components/plaza/plaza-*-view.tsx`、`plaza-feeds-item.tsx` | 原生面板 |
| `src/lib/workspace/navigation.ts` | `openPlazaSource` / `closePlazaTabs` / `openRemoteArxivPaper` |
| `src-tauri/src/features/paper/discovery/` | 站点代理、`feeds/`、`recommend/`、`coolpapers/`、`pwc/` 命令 |
