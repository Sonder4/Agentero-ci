(() => {
	try {
		// Only the top-level proxied document talks to the app. A nested frame on
		// the same proxy origin has a readable parent and stays uninstrumented.
		try {
			if (parent === window) return;
			void parent.location.href;
			return;
		} catch {}
		const post = (message) => {
			message.source = "agentero-web";
			parent.postMessage(message, "*");
		};
		// ---- selection → app toolbar -----------------------------------------
		const MAX_SELECTION_CHARS = 4000;
		const MAX_PAGE_CONTEXT_CHARS = 6000;
		const OMIT_FROM_PAGE_CONTEXT =
			"script, style, noscript, template, nav, aside, footer, form, input, textarea, select, button, [contenteditable], [inert], [aria-hidden=true], [hidden]";
		const canReadElement = (element) => {
			if (element.matches(OMIT_FROM_PAGE_CONTEXT)) return false;
			const style = getComputedStyle(element);
			return style.display !== "none" && style.visibility === "visible";
		};
		const readPage = (requestId) => {
			const root = document.querySelector("main, [role=main]") ?? document.body;
			let text = "";
			let previousBlock = null;
			if (
				root &&
				canReadElement(root) &&
				!root.closest(OMIT_FROM_PAGE_CONTEXT)
			) {
				const walker = document.createTreeWalker(
					root,
					NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT,
					{
						acceptNode(node) {
							if (node.nodeType === Node.TEXT_NODE)
								return NodeFilter.FILTER_ACCEPT;
							return canReadElement(node)
								? NodeFilter.FILTER_SKIP
								: NodeFilter.FILTER_REJECT;
						},
					},
				);
				for (
					let node = walker.nextNode();
					node && text.length <= MAX_PAGE_CONTEXT_CHARS;
					node = walker.nextNode()
				) {
					const part = node.textContent?.trim().replace(/\s+/g, " ");
					if (!part) continue;
					const block = node.parentElement?.closest(
						"p, li, h1, h2, h3, h4, h5, h6, pre, td, th, blockquote",
					);
					const separator = !text
						? ""
						: block && block !== previousBlock
							? "\n\n"
							: " ";
					text += (separator + part).slice(
						0,
						MAX_PAGE_CONTEXT_CHARS + 1 - text.length,
					);
					previousBlock = block;
				}
			}
			post({
				type: "pageContext",
				requestId,
				title: document.title.slice(0, 300),
				url: location.href,
				text: text
					.slice(0, MAX_PAGE_CONTEXT_CHARS)
					.replace(/[\uD800-\uDBFF]$/, ""),
				truncated: text.length > MAX_PAGE_CONTEXT_CHARS,
			});
		};
		const getSelectionGesture = (sel) => {
			if (!sel || sel.anchorNode == null || sel.focusNode == null) return {};
			try {
				const range = document.createRange();
				range.setStart(sel.anchorNode, sel.anchorOffset);
				range.collapse(true);
				const anchor = range.getBoundingClientRect();
				range.setStart(sel.focusNode, sel.focusOffset);
				range.collapse(true);
				const focus = range.getBoundingClientRect();
				return {
					side:
						focus.top > anchor.top
							? "below"
							: focus.top < anchor.top
								? "above"
								: undefined,
					focusX: focus.left,
				};
			} catch {
				return {};
			}
		};
		const sendSelection = (contextMenu) => {
			if (isEditable(document.activeElement)) return;
			let text = "";
			let rect = null;
			let side;
			let cursorX;
			const sel = window.getSelection();
			if (sel && sel.rangeCount > 0 && !sel.isCollapsed) {
				text = (sel.toString() || "").trim().slice(0, MAX_SELECTION_CHARS);
				if (text) {
					const r = sel.getRangeAt(0).getBoundingClientRect();
					if (r.width !== 0 || r.height !== 0) {
						rect = { x: r.left, y: r.top, width: r.width, height: r.height };
						const gesture = getSelectionGesture(sel);
						side = gesture.side;
						cursorX = gesture.focusX;
					}
				}
			}
			if (contextMenu) {
				if (
					contextMenu.defaultPrevented ||
					isEditable(contextMenu.target) ||
					((!text || !rect) &&
						contextMenu.target.closest?.(
							"a[href], img, video, audio, canvas, button",
						))
				)
					return;
				contextMenu.preventDefault();
			} else if (text && rect) {
				// Raw copy inside the frame: the app's clipboard write can be refused
				// while focus sits in this cross-origin frame.
				try {
					document.execCommand("copy");
				} catch {}
			}
			post({
				type: "selection",
				text: text,
				rect: rect,
				url: location.href,
				side: side,
				cursorX: cursorX,
				contextMenu: contextMenu
					? { x: contextMenu.clientX, y: contextMenu.clientY }
					: undefined,
			});
		};
		const scheduleSelection = () => {
			// Let the browser finish updating the selection first.
			requestAnimationFrame(() => {
				setTimeout(sendSelection, 0);
			});
		};
		document.addEventListener("mouseup", (event) => {
			if (event.button === 0) scheduleSelection();
		});
		document.addEventListener("contextmenu", sendSelection);
		document.addEventListener("keyup", (event) => {
			const key = event.key || "";
			if (
				key === "Shift" ||
				key.indexOf("Arrow") === 0 ||
				((event.metaKey || event.ctrlKey) && key.toLowerCase() === "a")
			)
				scheduleSelection();
		});

		// The app hides its toolbar while the page scrolls (plaza parity).
		let scrollPending = false;
		window.addEventListener(
			"scroll",
			() => {
				if (scrollPending) return;
				scrollPending = true;
				setTimeout(() => {
					scrollPending = false;
					post({ type: "scroll" });
				}, 150);
			},
			true,
		);

		// ---- shortcuts → app handlers -----------------------------------------
		// Keyboard events never bubble out of the frame. The app supplies effective
		// bindings and re-resolves forwarded keys against its live registry.
		let bindings = [];
		let isMac = false;
		const isEditable = (el) => {
			if (!el?.nodeName) return false;
			const name = el.nodeName.toLowerCase();
			return (
				name === "input" ||
				name === "textarea" ||
				name === "select" ||
				el.isContentEditable === true
			);
		};
		const matches = (event, def) => {
			let key = event.key || "";
			let defKey = def.key;
			if (key.length === 1) key = key.toLowerCase();
			if (defKey.length === 1) defKey = defKey.toLowerCase();
			const physicalKey = /^Key[A-Z]$/.test(event.code || "")
				? event.code.slice(3).toLowerCase()
				: null;
			if (
				key !== defKey &&
				event.key !== def.key &&
				!(def.alt && physicalKey === defKey)
			)
				return false;
			if (!!def.alt !== event.altKey || !!def.shift !== event.shiftKey)
				return false;
			if (isMac) {
				return !!def.meta === event.metaKey && !!def.ctrl === event.ctrlKey;
			}
			if (def.meta) return event.metaKey || event.ctrlKey;
			if (event.metaKey) return false;
			return !!def.ctrl === event.ctrlKey;
		};
		// Keyboard events never leave a cross-origin frame. Listen in the window
		// capture phase: SPA sites often stop propagation for their own overlays,
		// which would otherwise swallow app shortcuts and Escape. The site keeps
		// its handlers and default action; the app only closes UI it owns.
		window.addEventListener(
			"keydown",
			(event) => {
				if (event.isComposing || event.repeat || isEditable(event.target))
					return;
				if (event.key === "Escape") {
					if (!event.repeat) post({ type: "escape" });
					return;
				}
				if (event.defaultPrevented) return;
				for (let i = 0; i < bindings.length; i++) {
					const def = bindings[i];
					if (!matches(event, def)) continue;
					if (def.id !== "toggleChat") {
						const selection = window.getSelection();
						if (
							!selection ||
							selection.isCollapsed ||
							!selection.toString().trim()
						)
							return;
					}
					event.preventDefault();
					post({
						type: "shortcut",
						event: {
							key: event.key,
							code: event.code || "",
							metaKey: event.metaKey,
							ctrlKey: event.ctrlKey,
							altKey: event.altKey,
							shiftKey: event.shiftKey,
						},
					});
					return;
				}
			},
			true,
		);

		// ---- app → frame --------------------------------------------------------
		window.addEventListener("message", (event) => {
			const data = event.data;
			if (
				event.source !== parent ||
				!data ||
				data.source !== "agentero-web-host"
			)
				return;
			if (data.type === "shortcuts") {
				bindings = Array.isArray(data.bindings)
					? data.bindings.filter(
							(def) =>
								def &&
								typeof def.key === "string" &&
								(def.id === "toggleChat" ||
									def.id === "quickChat" ||
									def.id === "addSelectionToChat" ||
									def.id === "selectionTranslate" ||
									def.id === "selectionCopy"),
						)
					: [];
				isMac = data.isMac === true;
			} else if (data.type === "clearSelection") {
				const sel = window.getSelection();
				if (sel) sel.removeAllRanges();
			} else if (
				data.type === "readPage" &&
				typeof data.requestId === "string"
			) {
				readPage(data.requestId);
			}
		});
		post({ type: "ready" });
	} catch {}
})();
