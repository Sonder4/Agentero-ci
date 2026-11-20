(function runChatGptCli(slot, request) {
	if (location.origin !== "https://chatgpt.com") return false;
	globalThis[slot] = null;

	const PROJECT = /^\/g\/g-[a-z\d-]+\/(?:project\/?)?$/i;
	const CHAT = /^(?:\/g\/g-[a-z\d-]+)?\/c\/[a-z\d-]+\/?$/i;
	const CONTROL = 'a[href], button, [role=button], [role=menuitem], [role=tab]';
	const DIALOG = '[role=dialog], dialog[open]';
	const names = {
		pinned: /^(Pinned|置顶)$/,
		recent: /^(Recent|最近)$/,
		openSidebar: /^(Open sidebar|Show sidebar|打开侧边栏|显示侧边栏|展开侧边栏)$/i,
		closeSidebar: /^(Close sidebar|Hide sidebar|关闭侧边栏|隐藏侧边栏)$/i,
		newChat: /^(New chat|新聊天|新建聊天)$/i,
		newProject: /^(New project|Add (?:new )?project|添加新项目|新建项目|创建项目)$/i,
		rename: /^(Rename(?: chat)?|重命名)(?:\s+[RＲ])?$/i,
		pin: /^(Pin(?: chat)?|置顶(?:聊天)?)(?:\s+[PＰ])?$/i,
		unpin: /^(Unpin(?: chat)?|取消置顶(?:聊天)?)\b|^取消置顶(?:聊天)?$/i,
		move: /^(Move to project|移至项目|移动到项目)$/i,
		archive: /^(Archive(?: chat)?|归档)(?:\s+[AＡ])?$/i,
		share: /^(Share(?: chat)?|分享(?:聊天)?)$/i,
		shareLink: /^(Create link|Copy link|创建链接|复制链接)$/i,
		projectOptions: /^(?:Project(?: more)? (?:options|actions)|项目(?:侧边栏)?选项|项目操作|.+ 的项目操作|Options for .+)$/i,
		chatOptions: /^(Conversation (?:options|actions)|Chat(?: sidebar)? (?:options|actions)|聊天(?:侧边栏)?选项|聊天操作|对话操作|对话选项)$/i,
		projectRename: /^(Rename project|重命名项目)$/i,
		projectHome: /^(Project (?:home|homepage)|Go to project|Open project|项目主页|项目首页|打开项目)$/i,
		settings: /^(Project settings|项目设置)$/i,
		save: /^(Save(?: changes)?|Done|保存(?:更改)?|完成|确认)$/i,
		create: /^(Create(?: project)?|创建(?:项目)?|新建(?:项目)?)$/i,
		close: /^(Close(?: dialog)?|关闭(?:弹窗|对话框)?)$/i,
		chats: /^(Chats|聊天)$/,
		sources: /^(Sources|来源|资料来源)$/i,
		addSource: /^(Add sources?|添加来源|添加资料来源)$/i,
	};

	const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
	const clean = (value) => value?.trim().replace(/\s+/g, " ") ?? "";
	const shown = (item) =>
		item instanceof HTMLElement &&
		!item.closest("[inert], [hidden], [aria-hidden=true]") &&
		item.getClientRects().length > 0 &&
		getComputedStyle(item).visibility === "visible";
	const visible = (scope, selector) =>
		Array.from(scope?.querySelectorAll(selector) ?? []).filter(shown);
	const labels = (element) =>
		[element.getAttribute("aria-label"), element.title, element.innerText].map(clean);
	const byName = (scope, pattern) =>
		visible(scope, CONTROL).find((item) => labels(item).some((label) => pattern.test(label)));
	const isSidebarOverlay = (item) =>
		names.openSidebar.test(clean(item.getAttribute("aria-label"))) || Boolean(item.querySelector("nav[aria-label]"));
	const dialog = () => visible(document, DIALOG).filter((item) => !isSidebarOverlay(item)).at(-1);
	const menu = () => visible(document, "[role=menu]").at(-1);
	const main = () => document.querySelector("main");
	const projectPath = (path) => path.replace(/\/(?:project\/?)?$/, "");
	const isProject = (path) => typeof path === "string" && PROJECT.test(`${path.replace(/\/$/, "")}/`);
	const isChat = (path) => typeof path === "string" && CHAT.test(path);
	const same = (a, b) => isProject(a) && isProject(b)
		? projectPath(a) === projectPath(b)
		: a.replace(/\/$/, "") === b.replace(/\/$/, "");
	const activePath = () => location.pathname;
	const projectTarget = (value) => isProject(value) || (typeof value === "string" && value.startsWith("name:") && value.length > 5);
	const routeMatches = (target) => same(activePath(), target);
	const hasRoute = (item) => item instanceof HTMLAnchorElement && item.origin === location.origin;
	const routeTitle = (link) => clean(link.innerText?.split("\n").find((line) => line.trim()) || link.getAttribute("aria-label"));

	async function wait(find, reason, ms = 2800) {
		const deadline = performance.now() + ms;
		do {
			const result = find();
			if (result) return result;
			await sleep(100);
		} while (performance.now() < deadline);
		throw Error(reason);
	}

	function click(node, reason) {
		if (!node || node.matches(":disabled, [aria-disabled=true], [data-disabled]")) throw Error(reason);
		node.click();
	}

	async function openMenu(node) {
		if (!node) throw Error("The options button is unavailable");
		if (node.getAttribute("aria-expanded") !== "true" || !menu()) {
			node.dispatchEvent(new PointerEvent("pointerdown", {
				bubbles: true, cancelable: true, pointerType: "mouse", button: 0, buttons: 1,
			}));
			await sleep(80);
			if (!menu()) click(node, "The options button is disabled");
		}
		return wait(() => {
			const scope = menu();
			return visible(scope, "[role=menuitem]").some((item) => labels(item).some(Boolean)) && scope;
		}, "The options menu did not open");
	}

	const menuAction = (scope, name, reason) => wait(() => byName(menu() ?? scope, name), reason);

	function escape(scope) {
		const item = document.activeElement?.closest("[role=menu], [role=dialog]") ? document.activeElement : scope;
		item?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
	}

	function fieldValue(element, value) {
		const prototype = element instanceof HTMLTextAreaElement
			? HTMLTextAreaElement.prototype
			: HTMLInputElement.prototype;
		Object.getOwnPropertyDescriptor(prototype, "value").set.call(element, value);
		element.dispatchEvent(new Event("input", { bubbles: true }));
		element.dispatchEvent(new Event("change", { bubbles: true }));
	}

	function textValue({ allowEmpty = false } = {}) {
		if (typeof request.value !== "string" || (!allowEmpty && !request.value.trim()) || request.value.length > 24_000) {
			throw Error("Provide a value of at most 24000 characters");
		}
		return request.value;
	}

	function targetPath(predicate) {
		if (!predicate(request.target)) throw Error("Provide a matching relative ChatGPT target");
		return request.target;
	}

	async function sidebar() {
		const find = () => visible(document, "nav").find((nav) =>
			byName(nav, names.newChat) || byName(nav, names.recent) || nav.querySelector("a[href^='/c/']"),
		);
		if (!find()) {
			const opener = byName(document, names.openSidebar);
			if (opener) click(opener, "ChatGPT sidebar is unavailable");
		}
		return wait(find, "ChatGPT sidebar is unavailable or still loading", 5200);
	}

	async function closeSidebarOverlay() {
		const overlay = visible(document, DIALOG).find(isSidebarOverlay);
		if (!overlay) return;
		const close = byName(overlay, names.closeSidebar) ?? byName(document, names.closeSidebar);
		if (close) close.click();
		else escape(overlay);
		await wait(() => !shown(overlay), "The ChatGPT sidebar overlay did not close");
	}

	async function section(nav, name) {
		const marker = byName(nav, name);
		if (marker?.getAttribute("aria-expanded") === "false" || marker?.getAttribute("data-state") === "closed") {
			click(marker, "ChatGPT section is collapsed");
			await sleep(180);
		}
		return marker;
	}

	const follows = (item, marker) =>
		marker && Boolean(marker.compareDocumentPosition(item) & Node.DOCUMENT_POSITION_FOLLOWING);

	function links(scope, predicate) {
		return Array.from(scope?.querySelectorAll("a[href]") ?? []).filter((item) =>
			hasRoute(item) && predicate(item.pathname) && routeTitle(item) &&
			(shown(item) || Array.from(item.children).some(shown)),
		);
	}

	function entries(anchors) {
		return Array.from(new Map(anchors.map((link) => [
			link.pathname,
			{ title: routeTitle(link), path: link.pathname },
		])).values());
	}

	function projectRows(nav) {
		const named = [];
		for (const item of visible(nav, "button, [role=button]")) {
			const title = labels(item).map((label) => label.match(/^(.+?)\s+(?:的项目操作|project options|project actions)$/i)?.[1] ??
				label.match(/^Options for (.+)$/i)?.[1]).find(Boolean);
			if (title) named.push({ title, element: item, target: `name:${title}`, path: null });
		}
		for (const link of links(nav, isProject)) {
			const title = routeTitle(link);
			const matches = named.filter((row) => row.title === title && row.path === null);
			const row = { title, element: link, target: link.pathname, path: link.pathname };
			if (matches.length === 1) Object.assign(matches[0], row);
			else named.push(row);
		}
		return named;
	}

	function projectRow(nav, target) {
		const found = projectRows(nav).filter((row) => row.target === target ||
			(row.path && isProject(target) && same(row.path, target)) || target === `name:${row.title}`);
		if (found.length > 1) throw Error("More than one visible project matches the target");
		if (found.length === 0) throw Error("The target project is not visible in ChatGPT");
		return found[0];
	}

	function projectTitleMatches(title) {
		return visible(main(), "h1, h2").some((heading) => clean(heading.innerText) === title) ||
			visible(document, "button, [role=button]").some((button) =>
				!button.closest("nav") && labels(button).some((label) => [
					`${title} 的项目操作`, `更改 ${title} 的图标和颜色`, `Options for ${title}`, `${title} project options`,
				].includes(label)),
			);
	}

	async function openNamedProject(target) {
		const nav = await sidebar();
		const title = target.slice(5);
		const row = projectRow(nav, target);
		if (row.path) return openRoute(row.path);
		if (isProject(activePath()) && projectTitleMatches(title)) return;
		await openMenu(row.element);
		const home = await wait(() => byName(menu(), names.projectHome), "Project home is unavailable");
		click(home, "Project home is unavailable");
		await wait(() => isProject(activePath()) && !menu() && projectTitleMatches(title), "ChatGPT did not open the named project", 5000);
		await sleep(250);
	}

	async function openRoute(path) {
		if (!routeMatches(path)) {
			const link = visible(document, "a[href]").find((item) => hasRoute(item) && same(item.pathname, path));
			if (!link) throw Error("The requested link is not visible in ChatGPT");
			click(link, "The requested link cannot be opened");
			await wait(() => routeMatches(path), "ChatGPT did not open the requested link");
			await sleep(250);
		}
	}

	async function projectPage() {
		const path = targetPath(projectTarget);
		if (isProject(path)) {
			if (!routeMatches(path)) await sidebar();
			await openRoute(path);
		} else await openNamedProject(path);
		await closeSidebarOverlay();
		return wait(() => isProject(activePath()) && main(), "The project page is unavailable");
	}

	function optionsNear(link, pattern) {
		for (let row = link, depth = 0; row && depth < 5; row = row.parentElement, depth++) {
			const options = visible(row, "button, [role=button]").filter((button) =>
				labels(button).some((label) => pattern.test(label)) ||
				(button.matches("[aria-haspopup=menu]") && /options|more|选项|更多/i.test(labels(button).join(" "))),
			);
			if (options.length === 1) return options[0];
			if (options.length > 1) break;
		}
		return undefined;
	}

	async function chatMenu(target) {
		if (routeMatches(target)) {
			const header = main()?.querySelector("[data-app-shell-page-header]") ?? main();
			const options = byName(header, names.chatOptions);
			if (options) return openMenu(options);
		}
		const nav = await sidebar();
		const link = links(nav, isChat).find((item) => same(item.pathname, target)) ??
			links(main(), isChat).find((item) => same(item.pathname, target));
		const button = link && optionsNear(link, names.chatOptions);
		if (button) return openMenu(button);
		throw Error("The target conversation's options menu is unavailable");
	}

	async function projectMenu() {
		await projectPage();
		const button = await wait(() => visible(document, "button, [role=button]").find((item) =>
			!item.closest("nav") && labels(item).some((label) => names.projectOptions.test(label))),
		"The project options button is unavailable");
		return openMenu(button);
	}

	async function modalFields(selector) {
		return wait(() => visible(dialog(), selector)[0], "The expected ChatGPT editor is unavailable");
	}

	async function saveSettings(field) {
		const scope = dialog();
		const save = byName(scope, names.save);
		if (save && !save.disabled) click(save, "The settings could not be saved");
		else {
			// Settings can save on blur and apply the update when dismissed.
			field.blur();
			await dismissDialog(scope);
		}
		await wait(() => !dialog(), "ChatGPT did not close the settings dialog");
	}

	async function dismissDialog(scope) {
		const close = byName(scope, names.close);
		if (close) close.click();
		else escape(scope);
		await wait(() => dialog() !== scope, "The ChatGPT dialog did not close");
	}

	function projectChatsTab() {
		const scope = main();
		const sources = byName(scope, names.sources);
		for (let group = sources?.parentElement; group && group !== scope; group = group.parentElement) {
			const chats = byName(group, names.chats);
			if (chats) return chats;
		}
		return undefined;
	}

	function turnMessages() {
		const root = main();
		const markers = visible(root, "[data-conversation-role], [data-message-author-role]");
		const seen = new Set();
		return markers.flatMap((node, index) => {
			const role = node.dataset.conversationRole ?? node.dataset.messageAuthorRole;
			if (!["user", "assistant"].includes(role)) return [];
			const heading = node.matches("h4[data-conversation-role]");
			const container = heading ? node.parentElement : node;
			const selector = role === "user" ? "[data-user-message-bubble]" :
				"[data-response-body], [data-markdown-text-style], [data-dil-response], .markdown";
			const next = markers[index + 1];
			const body = container?.querySelector(selector) ??
				(heading ? Array.from(root.querySelectorAll(selector)).find((item) =>
					follows(item, node) && (!next || follows(next, item))) : node);
			if (!body || seen.has(body) || body.closest("[hidden], [inert]")) return [];
			seen.add(body);
			const text = (body.innerText || body.textContent).trim();
			const images = visible(body, "img").map((image) => ({ alt: image.alt }));
			const id = node.dataset.messageId ??
				body.querySelector("[data-chatgpt-selection-message-id]")?.dataset.chatgptSelectionMessageId ??
				body.querySelector("[data-dil-message-id]")?.dataset.dilMessageId ??
				body.closest("[data-chatgpt-search-message-ids]")?.dataset.chatgptSearchMessageIds?.match(
					/[a-f\d]{8}(?:-[a-f\d]{4}){3}-[a-f\d]{12}/i,
				)?.[0] ?? null;
			return text || images.length ? [{ id, role, text, images }] : [];
		});
	}

	const actions = {
		status: () => ({
			path: activePath(),
			...(request.value === "draft" ? {
				draft: visible(main(), '#prompt-textarea, form[data-chatgpt-composer] [contenteditable=true]')[0]?.innerText ?? null,
			} : {}),
			...(request.value === "structure" ? {
				messageRoles: Array.from(main()?.querySelectorAll("[data-conversation-role], [data-message-author-role]") ?? [], (marker) => ({
					role: marker.dataset.conversationRole ?? marker.dataset.messageAuthorRole,
					tag: marker.tagName,
					visible: shown(marker),
					bodyMarkers: marker.parentElement?.querySelectorAll("[data-user-message-bubble], [data-response-body], [data-markdown-text-style], [data-dil-response]").length ?? 0,
				})),
				projectChatLinks: (main()?.querySelectorAll("a[href*='/c/']").length ?? 0),
			} : {}),
			attachments: main()?.querySelectorAll(".composer-attachment-surface").length ?? 0,
			controls: visible(dialog() ?? menu() ?? document, CONTROL).flatMap((item) => {
				if (item instanceof HTMLAnchorElement && (isChat(item.pathname) || isProject(item.pathname))) return [];
				const [aria, title, text] = labels(item);
				const id = item.getAttribute("data-testid");
				return aria || title || id || item.matches('[role=menuitem]') ? [{
					label: aria || title || (item.matches('[role=menuitem]') ? text : ""),
					testId: id,
				}] : [];
			}).slice(0, 90),
			dialog: dialog()?.getAttribute("aria-label") || dialog()?.querySelector("h1,h2,h3")?.innerText || null,
			visibleLabels: visible(dialog() ?? menu() ?? main(), CONTROL).map((item) => labels(item)[2])
				.filter((label) => label && label.length <= 45).slice(0, 30),
			fields: visible(dialog() ?? main(), 'input, textarea, [contenteditable=true]').map((item) => ({
				id: item.id, type: item.getAttribute("type"), name: item.getAttribute("name"), testId: item.getAttribute("data-testid"),
			})),
		}),
		projects: async () => {
			const nav = await sidebar();
			const pinned = await section(nav, names.pinned);
			const other = await section(nav, /^Projects$|^项目$/i);
			const recent = byName(nav, names.recent);
			if (!pinned && !other && !recent) throw Error("ChatGPT project sections are unavailable");
			return {
				projects: projectRows(nav).map(({ element, ...entry }) => {
					return { ...entry, pinned: Boolean(follows(element, pinned) &&
						!(other && follows(element, other)) && !(recent && follows(element, recent))) };
				}),
			};
		},
		recent: async () => {
			const nav = await sidebar();
			const recent = await section(nav, names.recent);
			if (!recent) throw Error("The Recent section is unavailable");
			return { chats: entries(links(nav, isChat).filter((link) => follows(link, recent))) };
		},
		"project-chats": async () => {
			await projectPage();
			const read = () => entries(links(main(), isChat));
			let chats = read();
			if (chats.length === 0) {
				const tab = projectChatsTab();
				if (tab?.matches("button, [role=tab]")) tab.click();
				try {
					chats = await wait(() => {
						const found = read();
						return found.length > 0 && found;
					}, "No project chats are visible", 4300);
				} catch {
					chats = read();
				}
			}
			return { project: activePath(), chats };
		},
		"project-sources": async () => {
			const scope = await projectPage();
			const tab = await wait(() => byName(main(), names.sources), "The project's Sources tab is unavailable");
			if (tab.getAttribute("aria-selected") !== "true" && tab.getAttribute("data-state") !== "active") {
				click(tab, "The project's Sources tab is unavailable");
			}
			await wait(() => byName(main(), names.addSource), "The project source list did not load");
			return { project: activePath(), visibleText: (main() ?? scope).innerText };
		},
		open: async () => {
			const path = targetPath((value) => projectTarget(value) || isChat(value));
			if (!isChat(path) && !isProject(path)) await openNamedProject(path);
			else if (!routeMatches(path)) {
				await sidebar();
				await openRoute(path);
			}
			return { path: activePath() };
		},
		"new-chat": async () => {
			const nav = await sidebar();
			click(byName(nav, names.newChat), "New chat is unavailable");
			await wait(() => activePath() === "/" && main()?.querySelector("[contenteditable=true]"), "The new composer did not open");
			return { path: activePath() };
		},
		send: async () => {
			const value = textValue();
			if (request.target == null ? activePath() !== "/" : !routeMatches(targetPath(isChat))) {
				throw Error("The conversation has changed");
			}
			const userMessagesBefore = turnMessages().filter((item) => item.role === "user").length;
			const editor = visible(main(), "#prompt-textarea, form[data-chatgpt-composer] [contenteditable=true][role=textbox]")[0];
			const form = editor?.closest("form");
			if (!form || form.querySelector(".composer-attachment-surface") ||
				(editor.innerText.trim() && editor.innerText !== request.replaceDraft)) {
				throw Error("The composer is unavailable or contains a draft");
			}
			editor.focus();
			if (editor.innerText.trim()) {
				const range = document.createRange();
				range.selectNodeContents(editor);
				const selection = document.getSelection();
				selection.removeAllRanges();
				selection.addRange(range);
			}
			if (!document.execCommand("insertText", false, value)) throw Error("ChatGPT rejected the composer text");
			const submit = await wait(() => visible(form, "button").find((item) =>
				!item.disabled && (item.matches('[data-testid="send-button"], #composer-submit-button') ||
					labels(item).some((label) => /^(Send(?: prompt| message)?|发送(?:提示|消息)?)$/i.test(label))),
			), "The send control is unavailable");
			click(submit, "The message could not be sent");
			await wait(() => {
				const sent = turnMessages().filter((item) => item.role === "user");
				return isChat(activePath()) && sent.length > userMessagesBefore && sent.at(-1)?.text === value.trim();
			}, "ChatGPT did not confirm the sent message. Check the conversation before retrying.", 9000);
			return { path: activePath() };
		},
		"rename-chat": async () => {
			const target = targetPath(isChat);
			const value = textValue();
			const scope = await chatMenu(target);
			const rename = await menuAction(scope, names.rename, "Rename is unavailable for this conversation");
			click(rename, "Rename is unavailable for this conversation");
			const field = await wait(() => visible(dialog(), 'input:not([type=hidden])')[0] ??
				(document.activeElement instanceof HTMLInputElement && shown(document.activeElement) && document.activeElement),
			"The conversation name field did not open");
			fieldValue(field, value);
			const save = byName(dialog(), names.save);
			if (save) click(save, "The conversation could not be renamed");
			else {
				field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true }));
			}
			await wait(() => links(document, isChat).some((item) => same(item.pathname, target) && routeTitle(item) === value.trim()),
				"The renamed conversation did not appear");
			return { path: target, title: value };
		},
		"pin-chat": async () => {
			const target = targetPath(isChat);
			const scope = await chatMenu(target);
			const pin = await wait(() => byName(menu() ?? scope, names.pin) ||
				byName(menu() ?? scope, names.unpin), "Pin is unavailable for this conversation");
			if (labels(pin).some((label) => names.pin.test(label))) {
				click(pin, "Could not pin this conversation");
				await wait(() => !menu(), "The pin menu did not close");
			} else escape(scope);
			const nav = await sidebar();
			const pinned = await section(nav, names.pinned);
			const recent = byName(nav, names.recent);
			await wait(() => links(nav, isChat).some((item) => same(item.pathname, target) &&
				follows(item, pinned) && !(recent && follows(item, recent))), "The conversation did not appear in Pinned");
			return { path: target, pinned: true };
		},
		"move-chat": async () => {
			const target = targetPath(isChat);
			if (!projectTarget(request.value)) throw Error("Provide a project target as --value");
			const nav = await sidebar();
			const destination = projectRow(nav, request.value);
			const { title } = destination;
			const scope = await chatMenu(target);
			const move = await menuAction(scope, names.move, "Move to project is unavailable for this chat");
			move.focus();
			move.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, pointerType: "mouse" }));
			move.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
			if (visible(document, "[role=menu]").length === 1) move.click();
			const item = await wait(() => visible(menu(), "[role=menuitem]").find(
				(element) => labels(element).includes(title),
			), "The destination project did not appear in the move menu");
			click(item, "Could not move the conversation");
			await wait(() => !menu(), "The move menu did not close");
			await openNamedProject(`name:${title}`);
			const id = target.split("/").filter(Boolean).at(-1);
			await wait(() => links(main(), isChat).some((link) => link.pathname.endsWith(`/c/${id}`)),
				"The moved conversation did not appear on the project page", 6200);
			return { project: activePath(), path: links(main(), isChat).find((link) => link.pathname.endsWith(`/c/${id}`)).pathname };
		},
		"archive-chat": async () => {
			const target = targetPath(isChat);
			const scope = await chatMenu(target);
			const archive = await menuAction(scope, names.archive, "Archive is unavailable for this chat");
			click(archive, "Archive is unavailable for this chat");
			await sleep(400);
			if (dialog()) {
				click(byName(dialog(), names.archive), "Archive confirmation is unavailable");
			}
			await wait(() => !links(document, isChat).some((link) => same(link.pathname, target)),
				"The archived chat is still visible");
			return { path: target, archived: true };
		},
		"share-chat": async () => {
			const target = targetPath(isChat);
			const scope = await chatMenu(target);
			const share = await menuAction(scope, names.share, "Share is unavailable for this chat");
			click(share, "Share is unavailable for this chat");
			const modal = await wait(dialog, "The chat share dialog did not appear");
			const shareUrl = () => {
				const values = Array.from(modal.querySelectorAll("input, a[href]"), (item) => item.value ?? item.href);
				return values.find((value) => /^https:\/\/chatgpt\.com\/(?:share|s)\/[a-z\d_-]+\/?$/i.test(value ?? ""));
			};
			const button = await wait(() => byName(modal, names.shareLink), "Create share link is unavailable");
			click(button, "The share link could not be created");
			const result = await wait(() => shareUrl() ||
				visible(document, '[role=status], [role=alert]').some((item) => /link copied|链接已复制|已复制链接/i.test(item.innerText)),
			"ChatGPT did not confirm creation of the share link", 6500);
			if (dialog() === modal) {
				const close = byName(modal, names.close);
				if (close) close.click();
				else escape(modal);
			}
			return typeof result === "string" ? { path: target, shared: true, url: result } : { path: target, shared: true };
		},
		"create-project": async () => {
			const value = textValue();
			const nav = await sidebar();
			if (projectRows(nav).some((row) => row.title === value.trim())) throw Error("A project already has this name");
			const before = activePath();
			click(byName(nav, names.newProject), "New project is unavailable");
			const field = await modalFields('input:not([type=hidden])');
			fieldValue(field, value);
			click(byName(dialog(), names.create), "ChatGPT did not accept the project name");
			await wait(() => isProject(activePath()) && activePath() !== before && projectTitleMatches(value.trim()), "The new project did not open", 6800);
			return { path: activePath(), target: `name:${value.trim()}`, title: value };
		},
		"rename-project": async () => {
			const value = textValue();
			const scope = await projectMenu();
			click(byName(scope, names.projectRename) ?? byName(scope, names.settings), "Project rename is unavailable");
			const field = await modalFields('input:not([type=hidden])');
			fieldValue(field, value);
			await saveSettings(field);
			await wait(() => projectTitleMatches(value.trim()) ||
				projectRows(document).some((row) => row.title === value.trim()), "The renamed project did not appear");
			return { path: activePath(), title: value };
		},
		"project-instructions": async () => {
			const value = textValue({ allowEmpty: true });
			const scope = await projectMenu();
			click(byName(scope, names.settings), "Project settings are unavailable");
			const field = await modalFields("textarea");
			fieldValue(field, value);
			await saveSettings(field);
			const reopened = await projectMenu();
			click(byName(reopened, names.settings), "Project settings could not be reopened");
			const confirmed = await modalFields("textarea");
			if (confirmed.value !== value) throw Error("The project instructions were not saved");
			await dismissDialog(dialog());
			return { path: activePath(), saved: true };
		},
		"project-source": async () => {
			const file = request.file;
			if (!file || typeof file.name !== "string" || !file.name || /[/\\]/.test(file.name) ||
				typeof file.base64 !== "string" || !file.base64 || file.base64.length > 22_369_624) {
				throw Error("Provide a nonempty project source file of at most 16 MiB");
			}
			await projectPage();
			if (!byName(main(), names.addSource)) {
				const tab = await wait(() => byName(main(), names.sources), "The project's Sources tab is unavailable");
				click(tab, "The project's Sources tab is unavailable");
			}
			const add = await wait(() => byName(main(), names.addSource), "Add sources is unavailable");
			if (main().innerText.includes(file.name)) throw Error("A source with this file name is already visible");
			click(add, "The Add sources dialog could not open");
			const modal = await wait(dialog, "The Add sources dialog did not open");
			const inputs = Array.from(modal.querySelectorAll("input[type=file]:not(:disabled)"));
			const bytes = Uint8Array.from(atob(file.base64), (character) => character.charCodeAt(0));
			const transfer = new DataTransfer();
			transfer.items.add(new File([bytes], file.name));
			if (inputs.length === 1) {
				inputs[0].files = transfer.files;
				inputs[0].dispatchEvent(new Event("change", { bubbles: true }));
			} else {
				const drop = byName(modal, /Drop (?:sources|files)|将(?:来源|文件)拖到此处/i);
				if (!drop) throw Error("The project's file upload control is unavailable");
				drop.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, bubbles: true, cancelable: true }));
			}
			await wait(() => !dialog() && main()?.innerText.includes(file.name), "The source file did not appear", 22_000);
			return { path: activePath(), filename: file.name };
		},
		export: async () => {
			const target = targetPath(isChat);
			if (!routeMatches(target)) await sidebar();
			await openRoute(target);
			await closeSidebarOverlay();
			const read = () => {
				if (byName(main(), /^(Stop(?: generating| streaming)?|停止(?:生成)?)$/i)) {
					throw Error("Wait for the ChatGPT reply to finish before exporting");
				}
				return turnMessages();
			};
			let messages = await wait(() => {
				const rendered = read();
				return rendered.length > 0 && rendered;
			}, "No visible messages were found", 5200);
			if (messages.at(-1).role === "user") {
				// ChatGPT can render the prompt before hydrating the response on navigation.
				// A conversation can also end with a prompt that has no response.
				const deadline = performance.now() + 4000;
				do {
					await sleep(100);
					messages = read();
				} while (messages.at(-1)?.role !== "assistant" && performance.now() < deadline);
				if (!messages.length) throw Error("No visible messages were found");
			}
			return { path: activePath(), title: document.title, messages };
		},
	};

	Promise.resolve().then(() => {
		if (!Object.hasOwn(actions, request.operation)) throw Error("Unknown ChatGPT action");
		return actions[request.operation]();
	}).then(
		(result) => { globalThis[slot] = result; },
		(error) => { globalThis[slot] = { error: error instanceof Error ? error.message : "ChatGPT action failed" }; },
	);
	return true;
})
