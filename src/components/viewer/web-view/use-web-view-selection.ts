import { type RefObject, useCallback, useEffect, useRef } from "react";
import i18n from "@/i18n";
import {
	closeSelectionSource,
	dismissSelectionChat,
	openSelectionChat,
	selectionChatStore,
} from "@/lib/agent/selection-chat-store";
import { relaySelectionDismiss } from "@/lib/agent/selection-dismiss";
import {
	runSelectionAddToChat,
	runSelectionQuickChat,
} from "@/lib/agent/selection-quick-chat";
import { openChatGptSelectionMenu } from "@/lib/chatgpt/selection-menu";
import { openChatGptQuote } from "@/lib/chatgpt/window";
import { copyTextToClipboard } from "@/lib/core/clipboard";
import { notifyError } from "@/lib/core/notify";
import { openExternalUrl } from "@/lib/core/open-external";
import { getPlatformOS } from "@/lib/core/tauri";
import { settingsStore } from "@/lib/settings/react-store";
import { getShortcut, resolveShortcutId } from "@/lib/shell/shortcuts";
import { toggleChat } from "@/lib/shell/ui-window-actions";
import { webViewProxyOrigins } from "@/lib/web-view/proxy-url";
import {
	bridgeSelectionScreen,
	parseWebBridgeMessage,
	webBridgePageUrl,
} from "./bridge-message";

/** Frame adapter only: shared window host owns menus, requests and result cards. */
export function useWebViewSelection({
	srcUrl,
	iframeRef,
	title,
	origin,
	scale = 1,
}: {
	srcUrl: string;
	iframeRef: RefObject<HTMLIFrameElement | null>;
	title?: string;
	/** Explicit proxy origin, or "null" only for a sandboxed srcdoc frame. */
	origin?: string;
	scale?: number;
}) {
	const selectionSourceRef = useRef(srcUrl);
	const pendingPage = useRef<{ id: string; timer: number } | null>(null);
	const clearPendingPage = useCallback(() => {
		window.clearTimeout(pendingPage.current?.timer);
		pendingPage.current = null;
	}, []);
	const askPage = useCallback(() => {
		const frameWindow = iframeRef.current?.contentWindow;
		if (!frameWindow || pendingPage.current) return;
		const id = crypto.randomUUID();
		pendingPage.current = {
			id,
			timer: window.setTimeout(() => {
				clearPendingPage();
				notifyError(i18n.t("viewer:selection.pageContextUnavailable"));
			}, 2000),
		};
		frameWindow.postMessage(
			{ source: "agentero-web-host", type: "readPage", requestId: id },
			origin && origin !== "null" ? origin : "*",
		);
	}, [clearPendingPage, iframeRef, origin]);
	const syncShortcuts = useCallback(() => {
		const ownsSelection =
			selectionChatStore.getState().draft?.selection.sourcePath ===
			selectionSourceRef.current;
		iframeRef.current?.contentWindow?.postMessage(
			{
				source: "agentero-web-host",
				type: "shortcuts",
				isMac: getPlatformOS() === "macos",
				bindings: [
					...(["toggleChat", "quickChat", "addSelectionToChat"] as const)
						.map((id) =>
							getShortcut(id, settingsStore.getState().shortcutOverrides),
						)
						.filter(Boolean),
					...(ownsSelection
						? [
								{ id: "selectionTranslate", key: "t" },
								{ id: "selectionCopy", key: "c" },
							]
						: []),
				],
			},
			origin && origin !== "null" ? origin : "*",
		);
	}, [iframeRef, origin]);
	useEffect(() => {
		syncShortcuts();
		const unsubscribeSettings = settingsStore.subscribe(syncShortcuts);
		const unsubscribeSelection = selectionChatStore.subscribe((state, prev) => {
			if (state.draft !== prev.draft) syncShortcuts();
		});
		return () => {
			unsubscribeSettings();
			unsubscribeSelection();
		};
	}, [syncShortcuts]);
	useEffect(() => {
		let selectionSource = srcUrl;
		selectionSourceRef.current = srcUrl;
		const closeMenu = () => {
			const draft = selectionChatStore.getState().draft;
			if (draft && draft.selection.sourcePath === selectionSource)
				dismissSelectionChat();
		};
		const onMessage = (event: MessageEvent) => {
			const origins = origin ? [origin] : webViewProxyOrigins();
			if (
				!origins.includes(event.origin) ||
				event.source !== iframeRef.current?.contentWindow
			)
				return;
			const msg = parseWebBridgeMessage(event.data);
			if (!msg) return;
			if (msg.type === "ready") {
				syncShortcuts();
				return;
			}
			if (msg.type === "pageContext") {
				if (pendingPage.current?.id !== msg.requestId) return;
				clearPendingPage();
				const text = msg.text.trim();
				if (!text) {
					notifyError(i18n.t("viewer:selection.pageContextUnavailable"));
					return;
				}
				void openChatGptQuote(
					msg.truncated
						? `${text}\n\n[${i18n.t("viewer:selection.pageContextTruncated")}]`
						: text,
					{
						sourceUrl: webBridgePageUrl(srcUrl, msg.url, origins),
						sourceTitle: msg.title || title,
					},
				);
				return;
			}
			if (msg.type === "selection") {
				const iframe = iframeRef.current;
				if (!iframe) return;
				const bounds = iframe.getBoundingClientRect();
				if (!bounds.width || !bounds.height) return;
				const url = webBridgePageUrl(srcUrl, msg.url, origins);
				if (msg.contextMenu) {
					const { x, y } = msg.contextMenu;
					if (
						x < 0 ||
						y < 0 ||
						x * scale > bounds.width ||
						y * scale > bounds.height
					)
						return;
					openChatGptSelectionMenu({
						x: bounds.left + x * scale,
						y: bounds.top + y * scale,
						text: msg.rect ? msg.text : "",
						source: { sourceUrl: url, sourceTitle: title },
						askPage,
					});
					return;
				}
				if (!msg.text || !msg.rect) {
					closeMenu();
					return;
				}
				if (selectionSource !== url) closeSelectionSource(selectionSource);
				selectionSource = url;
				selectionSourceRef.current = url;
				openSelectionChat(
					{
						text: msg.text,
						sourcePath: url,
						sourceUrl: url,
						sourceTitle: title,
						origin: "markdown",
						surface: "web",
					},
					bridgeSelectionScreen(
						{
							x: msg.rect.x * scale,
							y: msg.rect.y * scale,
							width: msg.rect.width * scale,
							height: msg.rect.height * scale,
						},
						bounds,
						msg.side,
						msg.cursorX != null ? msg.cursorX * scale : undefined,
					),
				);
				void copyTextToClipboard(msg.text);
			} else if (msg.type === "scroll") closeMenu();
			else if (msg.type === "escape") relaySelectionDismiss();
			else if (msg.type === "shortcut") {
				if (
					selectionChatStore.getState().draft?.selection.sourcePath ===
						selectionSource &&
					["t", "c"].includes(msg.event.key.toLowerCase()) &&
					!msg.event.metaKey &&
					!msg.event.ctrlKey &&
					!msg.event.altKey &&
					!msg.event.shiftKey
				) {
					// Run the same mounted toolbar action and copied feedback as a click.
					window.dispatchEvent(
						new KeyboardEvent("keydown", { ...msg.event, cancelable: true }),
					);
					return;
				}
				const id = resolveShortcutId(new KeyboardEvent("keydown", msg.event), {
					settingsOpen: false,
				});
				if (id === "toggleChat") toggleChat();
				else if (
					selectionChatStore.getState().draft?.selection.sourcePath ===
					selectionSource
				) {
					if (id === "quickChat") runSelectionQuickChat();
					else if (id === "addSelectionToChat" && runSelectionAddToChat())
						iframeRef.current?.contentWindow?.postMessage(
							{ source: "agentero-web-host", type: "clearSelection" },
							"*",
						);
				}
			} else if (msg.type === "external") openExternalUrl(msg.url);
		};
		window.addEventListener("message", onMessage);
		return () => {
			window.removeEventListener("message", onMessage);
			clearPendingPage();
			closeSelectionSource(selectionSource);
		};
	}, [
		srcUrl,
		iframeRef,
		title,
		origin,
		scale,
		syncShortcuts,
		askPage,
		clearPendingPage,
	]);
	return { onFrameLoad: syncShortcuts, askPage };
}
