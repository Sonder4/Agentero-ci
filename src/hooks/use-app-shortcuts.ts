import { useEffect, useRef } from "react";
import { useNativeSelectAllGuard } from "@/hooks/use-native-select-all-guard";
import { registerDesktopShortcuts } from "@/lib/shell/commands/desktop-ui-actions";
import {
	formatShortcut,
	getShortcut,
	resolveShortcutId,
	SHORTCUTS,
	type ShortcutId,
	shortcutBelongsToTextField,
} from "@/lib/shell/shortcuts";

/** One handler per global keyboard shortcut. */
export type ShortcutHandlers = Record<ShortcutId, () => void>;

/**
 * Shortcuts that act on the vault file tree and make no sense anywhere else.
 * They fire only while the tree is the surface the user last touched, so the
 * keystroke can fall through to native cut/paste on other surfaces.
 */
const TREE_SURFACE_SHORTCUTS: ReadonlySet<ShortcutId> = new Set([
	"deleteTreeItem",
	"collapseTreeCurrent",
	"collapseTreeDefault",
	"cutTreeItem",
	"pasteTreeItem",
]);

/**
 * Bind the global keyboard shortcuts once and dispatch each to its handler.
 * Handlers are read from a ref so the listener never needs to re-bind.
 *
 * @param modalOverlayOpen - a modal app overlay/sheet is open (settings, dialogs, palette…).
 *   Gates `whenSettingsOpen` / `whenSettingsClosed` shortcut rules. Docked, non-modal
 *   surfaces such as the Agent ask-user form do not gate shortcuts.
 */
export function useAppShortcuts(
	modalOverlayOpen: boolean,
	handlers: ShortcutHandlers,
): void {
	useNativeSelectAllGuard();

	const modalOverlayOpenRef = useRef(modalOverlayOpen);
	modalOverlayOpenRef.current = modalOverlayOpen;
	const handlersRef = useRef(handlers);
	handlersRef.current = handlers;

	// Whether the vault file tree was the surface the user last interacted
	// with. Its clipboard shortcuts (⌘X / ⌘V) may only claim the keystroke on
	// its own surface: everywhere else the event must fall through so the
	// platform emits the native cut/paste — canvases and other viewers paste
	// clipboard content only from a real `paste` event, and preventing the
	// keydown suppresses it. WebKit does not focus <button> rows on click, so
	// the keydown target / activeElement is unreliable; track pointer + focus
	// instead.
	const treeSurfaceActiveRef = useRef(false);
	useEffect(() => {
		const withinTree = (target: EventTarget | null) =>
			target instanceof Element && target.closest("[data-file-tree]") != null;
		const onPointerDown = (event: PointerEvent) => {
			treeSurfaceActiveRef.current = withinTree(event.target);
		};
		const onFocusIn = (event: FocusEvent) => {
			treeSurfaceActiveRef.current = withinTree(event.target);
		};
		document.addEventListener("pointerdown", onPointerDown, true);
		document.addEventListener("focusin", onFocusIn, true);
		return () => {
			document.removeEventListener("pointerdown", onPointerDown, true);
			document.removeEventListener("focusin", onFocusIn, true);
		};
	}, []);

	useEffect(() => {
		const canRun = (
			id: ShortcutId,
			target: EventTarget | null,
			key: string,
		) => {
			if (document.querySelector("[data-shortcut-recording]")) return false;
			if (target instanceof Element && target.closest("[data-shortcut-search]"))
				return false;
			if (shortcutBelongsToTextField(id, { key, target })) return false;
			return !TREE_SURFACE_SHORTCUTS.has(id) || treeSurfaceActiveRef.current;
		};
		const context = (id: ShortcutId) => {
			const binding = getShortcut(id);
			const overlayOpen = modalOverlayOpenRef.current;
			return {
				key: binding ? formatShortcut(binding) : "",
				enabled: Boolean(
					binding &&
						(!binding.whenSettingsOpen || overlayOpen) &&
						(!binding.whenSettingsClosed || !overlayOpen) &&
						canRun(id, document.activeElement, binding.key),
				),
			};
		};
		const unregister = registerDesktopShortcuts({
			list: () => SHORTCUTS.map(({ id }) => ({ id, ...context(id) })),
			run: (id) => {
				const def = SHORTCUTS.find((shortcut) => shortcut.id === id);
				if (!def || !context(def.id).enabled)
					throw new Error(
						`Shortcut is unavailable in the current context: ${id}`,
					);
				handlersRef.current[def.id]();
			},
		});
		const onKeyDown = (event: KeyboardEvent) => {
			const id = resolveShortcutId(event, {
				settingsOpen: modalOverlayOpenRef.current,
			});
			if (!id) return;

			// Text fields keep their editor-native combos — ⌘⌫ delete-to-line,
			// ⌘← / ⇧⌘← line jumps, ⌘X/⌘V native cut/paste, ⌥A composed chars,
			// ⌘B bold. The shared helper owns that id/target decision.
			if (!canRun(id, event.target, event.key)) return;

			event.preventDefault();
			if (id === "toggleMarkdownSource") {
				event.stopPropagation();
				if (event.repeat) return;
			}
			handlersRef.current[id]();
		};
		window.addEventListener("keydown", onKeyDown, true);
		return () => {
			unregister();
			window.removeEventListener("keydown", onKeyDown, true);
		};
	}, []);
}
