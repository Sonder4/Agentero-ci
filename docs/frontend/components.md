# 组件约定

## AI Elements

Chat / Agent UI 使用 [shadcn/ui AI Elements](https://ui.shadcn.com/docs/ai-elements) 的 Radix patterns，并复用当前 `radix-nova` 基础组件。

- 上游基础件落盘：`src/components/ui/`。安装前用 `pnpm exec shadcn view <name>` 对比，按需添加，禁止批量覆盖已有组件。
- 应用组合位于 `src/components/agent/`：`elements/` 为过程展示与队列，`composer/` 为输入状态与附件，`rendering/` 为科研 Markdown、引用及代码展示。旧 `ai-elements/` 目录已移除。
- 主题跟 shadcn token；**不是** Vercel AI SDK `useChat`（传输层为 ACP）。
- `ui/message`、`bubble`、`trace`、`code-block`、`attachment`、`message-scroller` 保持上游通用接口，通过 props / children 组合业务；不导入 ACP、Vault、PDF、业务 store 或 IPC。`pnpm deps:check` 检查这条边界。
- 保留 AI SDK、Streamdown 等成熟依赖；只有确认无消费者才删除，不为减少依赖重新实现基础能力。

待发送队列与官方 Agent todo Queue 语义不同；模型选择器需要 Command 搜索；Vault 文件树需要多选、拖放和重命名。这些能力用基础组件组合，不能用精简展示示例替代。

提问表单使用 `ui/questionnaire`，分页、选择、自由输入、校验与焦点由 `@shadcn/react/questionnaire` 管理；业务只负责 ACP 问题/答案转换、折叠和异步提交恢复。该基础件按官方 Radix 源码与 Nova style 展开，禁止在基础件里处理协议回传。

待发送与已发送图片均使用 Attachment；已发送图片与视觉批注共用 `agent/chat-image-gallery.tsx` 的预览交互。代码块复用 Code Block 的 Header / Title / Actions，Tauri 剪贴板与文件下载保留在业务层。

引用来源列表用 Trace Sources 组合并按 URI 去重；行内引用用 Hover Card + Button/链接组合，Vault/PDF 跳转仍经项目解析器。Terminal pattern 只接受可验证的 ACP execute 命令、可选 cwd 与文本输出，保留完整 metadata；未知字段回退通用 Tool，退出码只读结构化字段，不能从日志猜测或隐藏参数。

Agent / Reasoning 消息里的 Markdown：`Streamdown` + `PlainTable` / `PlainCodeBlock`（代码块左上角语言标签，悬停复制/下载；无外侧卡片，见 [agent.md](agent.md)#消息-markdown-渲染）。

## 表面与层级

浮层统一走主题阴影阶梯，不在业务组件里写 `shadow-lg` / `shadow-2xl`：

| Token | 用途 |
|---|---|
| `shadow-raised` | 可悬浮的卡片 / 按钮 hover，轻量抬升 |
| `shadow-overlay` | Popover、Dropdown、ContextMenu、Select、Tooltip、Toast、PDF 浮卡 |
| `shadow-dialog` | Dialog / Command 主模态 |

阴影值定义在 `src/index.css`（`--agentero-shadow-*`，暗色单独增强）；浮层仍保留 `ring-1 ring-foreground/10` 发丝边，白底时不靠阴影硬撑边界。菜单容器用 `p-1.5`、菜单项用 `px-2 py-1.5`，与 `Button` 的 24 / 28 / 32 / 36 控件高度阶梯对齐；以屏幕坐标锚定的自绘菜单走 `FloatingMenu`，与 `ContextMenuContent` 使用同一表面。

## 复用层

浮层、列表行和编辑单元格先组合下列共享件，不在业务目录重写同一条交互：

| 组件 | 位置 | 用途 |
|---|---|---|
| `SelectionToolbar` + `SelectionToolbarAction` / `SelectionToolbarDivider` / `SelectionToolbarIconButton` | `components/selection/selection-toolbar.tsx` | PDF 文本选区、Plaza 选区、高亮标注菜单共用高度、描边、缩放与 dim 行为 |
| `FloatingMenu` + `FloatingMenuItem` / `FloatingMenuShortcut` / `FloatingMenuSeparator` | `components/ui/floating-menu.tsx` | 屏幕坐标锚定的菜单（文件树右键、Plaza 卡片）；行距与快捷键对齐 Radix 菜单 |
| `PanelCard` + `PanelCardButton` / `PanelCardActions` / `PanelEmptyState` | `components/viewer/panels/panel-primitives.tsx` | PDF 右栏卡片、整卡跳转、hover 操作条与空态 |
| `QuietIconButton` | `components/ui/quiet-icon-button.tsx` | 24px 静音图标按钮（面板头、hover 操作条、PDF 浮层） |
| `EmptyState` | `components/ui/empty-state.tsx` | 查看器无内容时的居中提示 |
| `WindowHeader` | `components/shell/window-header.tsx` | 主窗口 / 功能窗口 / 文档窗口共用的 32px 标题栏 |
| `EDITABLE_CELL` | `components/library/cell-styles.ts` | 论文库自定义属性的文本 / 下拉编辑单元格框 |

新增浮层或列表卡片先查这些入口。行为（键盘、ARIA、定位）重复时抽组件；只有样式重复时用常量或 token。

## 交互态

列表行（论文库、PDF 目录、设置导航、文件树）的 hover / 选中底色落在整行容器上。只读单元格不再叠加内容宽度的 hover 底色；可编辑单元格用尾随铅笔 / 下拉图标提示，行底色由表格行统一提供。PDF 书签编辑行把 hover 放在整行 `<fieldset>` 上，尾部操作按钮也共享同一底色。设置页侧栏的选中态保持平层，不加阴影。

## 可脚本化控件

要求：每个可交互控件都能被 `agentero ui` 发现并触发（总则见根目录 `AGENTS.md`「UI 可脚本化」，命令用法见 [../backend/cli.md](../backend/cli.md#触发运行中窗口的控件)）。

**发现规则**（`src/lib/shell/commands/desktop-ui-actions.ts`）：

- 候选元素：`[data-ui-action]`、`button`、可点击的 `input`、文本输入框、`textarea`、`summary`、`a[href]`、`[contenteditable=true]`、`[data-hint]`，以及 role 为 `button` / `checkbox` / `switch` / `radio` / `tab` / `combobox` / `menuitem*` / `option` / `treeitem` / `textbox` 的元素。
- 排除：位于 `aria-hidden=true`、`inert`、`aria-disabled=true`、`hidden` 内，`:disabled` 或 `data-disabled`，`visibility` 不是 `visible`，不在视口内，或中心点被其它元素遮住的控件。
- 范围：焦点在菜单 / 列表框里时只看该菜单；有模态对话框时只看对话框；否则看整个窗口。
- 名称取值顺序：`aria-labelledby` → `aria-label` → `<label>` / 按钮值 → 可见文字 → `title` → `placeholder`；都没有时显示为 `[role]`。同名控件按出现顺序编号，用 `--occurrence` 选择。下拉框用 `aria-label`、`aria-labelledby` 或与触发按钮关联的 `<label>` 命名，名称随其用途保持稳定。

**组件写法**：

- 用 shadcn / Radix 组件或原生元素，不要用无 role 的 `div` 承接点击。
- 只有图标的按钮必须有 `aria-label`（文案走 `t()`），可同时给 `title`。
- 测试流程里会用到的控件加 `data-ui-action`，命名为 `<域>.<对象>.<动作>`，如 `chatgpt.selection.ask`、`pdf.viewport`。同一时刻同一 ID 只出现一处可见控件；列表行等重复控件靠名称加 `occurrence` 区分，或在 ID 里带上稳定的业务键。
- 只在悬停时渲染的入口（如面包屑的子项候选）、拖拽、画布上的手势，要在命令面板、快捷键或 CLI 子命令中提供等价操作。
- 操作完成后，界面状态要能被下一次 `ui actions` 观察到（出现新控件、标签、选中态）；失败走 `notifyError` 并写日志。

## 业务组件目录

| 目录 | 职责 |
|---|---|
| `components/shell/` | 三栏壳、标题栏、对话框挂载 |
| `components/sidebar/` | 文件树、Paper Info、LaTeX 面板、魔棒 |
| `components/writing/` | 写作工作面：阅读 / 写作切换、项目列表、源码与 PDF |
| `components/workspace/` | Dockview 宿主与 panel 内容 |
| `components/library/` | 论文库表 |
| `components/editor/` | Plate Markdown |
| `components/viewer/` | PDF / HTML / 图片。对外只经 `viewer/index.ts`；内部 `pdf/`（外壳 + `hooks/` + `layers/` + `chrome/` + `cards/` + `viewport/`）、`panels/`（右栏面板） |
| `components/agent/` | Agent 面板。外壳 + `hooks/`（面板与 composer 状态）+ `composer/`（输入区子件） |
| `components/wiki/` | 反链列表 / 引用近邻图（`graph-panel`） |
| `components/settings/` | 设置页 |
| `components/dialogs/` | 命令面板、权限、迁移等 |
| `components/ui/` | shadcn 基础件；`math-text.tsx` 用 KaTeX 渲染标题内联公式（`$\\pi$`） |

## 状态订阅

组件经 `hooks/use-app-stores.ts` selector 订阅；避免整树重渲染。命令式 dock/布局经 registry，不把 DOM ref 塞进 zustand。
