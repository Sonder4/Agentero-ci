import { useEffect } from "react";
import { commands, events, type UiRequestPayload } from "@/lib/core/bindings";
import { errorText } from "@/lib/core/error";
import { logger } from "@/lib/core/logger";
import { isMobileApp, isTauri } from "@/lib/core/tauri";
import { activateHintTarget } from "@/lib/keyboard/hint-activate";
import { topKeyboardModal } from "@/lib/keyboard/ownership";
import { listCommands } from "@/lib/shell/commands/registry";

type DesktopShortcut = {
	id: string;
	key: string;
	enabled: boolean;
};

type ShortcutActions = {
	list: () => DesktopShortcut[];
	run: (id: string) => void;
};

let shortcutActions: ShortcutActions | undefined;
const LOCAL_ACTIONS = new Set([
	"actions",
	"shortcut",
	"command",
	"click",
	"key",
	"fill",
]);

export function isDesktopUiAction(action: string): boolean {
	return LOCAL_ACTIONS.has(action);
}

export function registerDesktopShortcuts(actions: ShortcutActions): () => void {
	shortcutActions = actions;
	return () => {
		if (shortcutActions === actions) shortcutActions = undefined;
	};
}

const TEXT_INPUT_SELECTOR =
	"input:not([type]), input[type=text], input[type=search], input[type=url], input[type=email], input[type=number], input[type=tel]";

const CONTROL_SELECTOR = [
	"[data-ui-action]",
	"button",
	"input[type=button]",
	"input[type=submit]",
	"input[type=checkbox]",
	"input[type=radio]",
	TEXT_INPUT_SELECTOR,
	"summary",
	"[role=button]",
	"[role=checkbox]",
	"[role=switch]",
	"[role=radio]",
	"[role=tab]",
	"[role=combobox]",
	"[role=menuitem]",
	"[role=menuitemcheckbox]",
	"[role=menuitemradio]",
	"[role=option]",
	"[role=treeitem]",
	"[role=textbox]",
	"select",
	"[contenteditable=true]",
	"textarea",
	"a[href]",
	"[data-hint]",
].join(",");

function controlLabel(element: HTMLElement): string {
	const labelledBy = element
		.getAttribute("aria-labelledby")
		?.trim()
		.split(/\s+/)
		.map((id) => document.getElementById(id)?.textContent)
		.filter(Boolean)
		.join(" ");
	const inputLabel =
		"labels" in element
			? Array.from(
					(element as HTMLInputElement).labels ?? [],
					(label) => label.textContent,
				).join(" ")
			: "";
	return (
		labelledBy ||
		element.getAttribute("aria-label") ||
		inputLabel ||
		(element instanceof HTMLInputElement &&
			element.matches("[type=button], [type=submit]") &&
			element.value) ||
		(!element.isContentEditable &&
			!(element instanceof HTMLTextAreaElement) &&
			element.innerText) ||
		element.title ||
		element.getAttribute("placeholder") ||
		""
	)
		.trim()
		.replace(/\s+/g, " ");
}

function controlPoint(element: HTMLElement) {
	const rect = element.getBoundingClientRect();
	let left = Math.max(0, rect.left);
	let right = Math.min(document.documentElement.clientWidth, rect.right);
	let top = Math.max(0, rect.top);
	let bottom = Math.min(document.documentElement.clientHeight, rect.bottom);
	const clips = /^(auto|scroll|hidden|clip|overlay)$/;
	for (
		let parent = element.parentElement;
		parent;
		parent = parent.parentElement
	) {
		const { overflowX, overflowY } = getComputedStyle(parent);
		if (!clips.test(overflowX) && !clips.test(overflowY)) continue;
		const bounds = parent.getBoundingClientRect();
		if (clips.test(overflowX)) {
			left = Math.max(left, bounds.left);
			right = Math.min(right, bounds.right);
		}
		if (clips.test(overflowY)) {
			top = Math.max(top, bounds.top);
			bottom = Math.min(bottom, bounds.bottom);
		}
	}
	if (left >= right || top >= bottom) return undefined;
	for (const y of [0.5, 0.1, 0.9]) {
		for (const x of [0.5, 0.1, 0.9]) {
			const point = {
				x: left + (right - left) * x,
				y: top + (bottom - top) * y,
			};
			if (element.contains(document.elementFromPoint(point.x, point.y)))
				return point;
		}
	}
}

function availableControls() {
	const active = document.activeElement;
	const modal = topKeyboardModal(document);
	const scope =
		active?.closest("[role=menu], [role=listbox]") ??
		(modal?.getAttribute("aria-modal") !== "false" ? modal : null) ??
		document;
	return Array.from(
		scope.querySelectorAll<HTMLElement>(CONTROL_SELECTOR),
	).flatMap((element) => {
		if (
			element.closest(
				"[aria-hidden=true], [inert], [aria-disabled=true], [hidden], [data-disabled]:not([data-disabled=false])",
			) ||
			element.matches(":disabled") ||
			getComputedStyle(element).visibility !== "visible" ||
			!element.getClientRects().length
		)
			return [];
		const label =
			controlLabel(element) ||
			`[${element.getAttribute("role") ?? element.localName}]`;
		return [{ id: element.dataset.uiAction, label, element }];
	});
}

function visibleControls(controls = availableControls()) {
	const counts = new Map<string, number>();
	return controls.flatMap((control) => {
		const point = controlPoint(control.element);
		if (!point) return [];
		const key = control.label;
		const occurrence = (counts.get(key) ?? 0) + 1;
		counts.set(key, occurrence);
		return [{ ...control, occurrence, point }];
	});
}

async function clickControl(
	target: string,
	index: string | null | undefined,
	right: boolean,
) {
	const findControls = () => {
		const controls = availableControls();
		const byId = controls.filter((control) => control.id === target);
		const candidates = byId.length
			? byId
			: controls.filter((control) => control.label === target);
		const visible = visibleControls(candidates);
		const candidate = candidates[0];
		if (
			!visible.length &&
			candidates.length === 1 &&
			candidate &&
			index == null
		) {
			candidate.element.scrollIntoView({
				block: "nearest",
				inline: "nearest",
				behavior: "instant",
			});
			return visibleControls(candidates);
		}
		return visible;
	};
	let matches = findControls();
	const deadline = Date.now() + 2000;
	while (!matches.length && Date.now() < deadline) {
		await new Promise((resolve) => setTimeout(resolve, 125));
		matches = findControls();
	}
	if (matches.length > 1 && index == null)
		throw new Error(
			`Multiple controls match "${target}". Choose --occurrence from ui actions.`,
		);
	const control = matches[Number(index ?? 1) - 1];
	if (!control) throw new Error(`No enabled control matches "${target}".`);
	if (right) {
		control.element.dispatchEvent(
			new MouseEvent("contextmenu", {
				bubbles: true,
				cancelable: true,
				button: 2,
				clientX: control.point.x,
				clientY: control.point.y,
			}),
		);
		return;
	}
	activateHintTarget(control.element, control.point);
}

function dispatchKey(chord: string) {
	const parts =
		chord === "+" ? ["+"] : chord.replace(/\+\+$/, "+Plus").split("+");
	const rawKey = parts.pop()?.trim();
	const modifiers = new Set(
		parts.map((modifier) => modifier.trim().toLowerCase()),
	);
	if (
		!rawKey ||
		[...modifiers].some(
			(mod) => !["ctrl", "alt", "shift", "meta"].includes(mod),
		)
	)
		throw new Error("Expected a key chord such as Ctrl+f or Shift+ArrowRight.");
	const key = rawKey === "Plus" ? "+" : rawKey;
	const code = /^[a-z]$/i.test(key)
		? `Key${key.toUpperCase()}`
		: /^\d$/.test(key)
			? `Digit${key}`
			: key;
	const target = document.activeElement ?? document.body;
	if (target instanceof HTMLIFrameElement)
		throw new Error("The focused frame handles its own keyboard input.");
	const init = {
		key,
		code,
		bubbles: true,
		cancelable: true,
		ctrlKey: modifiers.has("ctrl"),
		metaKey: modifiers.has("meta"),
		shiftKey: modifiers.has("shift"),
		altKey: modifiers.has("alt"),
	};
	const allowDefault = target.dispatchEvent(new KeyboardEvent("keydown", init));
	if (
		allowDefault &&
		key.toLowerCase() === "a" &&
		(init.ctrlKey || init.metaKey) &&
		!init.altKey &&
		!init.shiftKey
	) {
		if (
			target instanceof HTMLTextAreaElement ||
			(target instanceof HTMLInputElement &&
				target.matches("[type=text], [type=search], input:not([type])"))
		) {
			target.select();
		} else if (target instanceof HTMLElement && target.isContentEditable) {
			document.execCommand("selectAll");
		}
	}
	target.dispatchEvent(new KeyboardEvent("keyup", init));
}

function fillFocusedInput(value: string): void {
	const field = document.activeElement;
	const prototype =
		field instanceof HTMLTextAreaElement
			? HTMLTextAreaElement.prototype
			: field?.matches(TEXT_INPUT_SELECTOR)
				? HTMLInputElement.prototype
				: null;
	if (!prototype || !field || field.matches(":disabled, [readonly]")) {
		throw new Error(
			"Focus an editable Agentero text field before using ui fill.",
		);
	}
	Object.getOwnPropertyDescriptor(prototype, "value")?.set?.call(field, value);
	field.dispatchEvent(new Event("input", { bubbles: true }));
}

async function runDesktopAction(request: UiRequestPayload) {
	const target = request.path ?? "";
	switch (request.action) {
		case "actions": {
			const shortcuts = shortcutActions?.list() ?? [];
			const registered = listCommands()
				.filter((command) => command.when?.() !== false)
				.map(({ id }) => id);
			const controls = visibleControls().map(({ id, label, occurrence }) => ({
				id,
				label,
				occurrence,
			}));
			return {
				shortcuts,
				commands: registered,
				controls,
				lines: [
					...shortcuts.map(
						({ id, key, enabled }) =>
							`shortcut ${id}${enabled ? ` (${key})` : " (unavailable)"}`,
					),
					...registered.map((id) => `command  ${id}`),
					...controls.map(
						({ id, label, occurrence }) =>
							`click    ${id ? `${id} · ` : ""}${label} [${occurrence}]`,
					),
				],
			};
		}
		case "shortcut":
			if (!shortcutActions)
				throw new Error("This window has no global shortcuts.");
			shortcutActions.run(target);
			break;
		case "command": {
			const command = listCommands().find(({ id }) => id === target);
			if (!command || command.when?.() === false)
				throw new Error(`Command is unavailable: ${target}`);
			await command.run();
			break;
		}
		case "click":
			await clickControl(
				target,
				request.section,
				request.direction === "right",
			);
			break;
		case "key":
			dispatchKey(target);
			break;
		case "fill":
			fillFocusedInput(target);
			return {
				action: request.action,
				window: request.window,
				lines: ["Filled the focused input"],
			};
		default:
			throw new Error("Unknown desktop action.");
	}
	return {
		action: request.action,
		target,
		window: request.window,
		lines: [`Dispatched ${request.action} ${target}`],
	};
}

/** Mount in each local renderer after its initial UI is committed. */
export function DesktopUiActionListener() {
	useEffect(() => {
		if (!isTauri() || isMobileApp()) return;
		let disposed = false;
		let stop: (() => void) | undefined;
		let pending = Promise.resolve();
		const drain = () => {
			pending = pending
				.then(async () => {
					if (disposed) return;
					for (const request of await commands.uiActionTakePending()) {
						if (!request.reference) continue;
						const result = await runDesktopAction(request).catch((error) => ({
							error: errorText(error),
						}));
						const reply = await commands.uiActionComplete(
							request.reference,
							JSON.stringify(result),
						);
						if (reply.status === "error") logger.error(reply.error);
					}
				})
				.catch((error) => logger.error(errorText(error)));
		};
		void import("@tauri-apps/api/webview")
			.then(({ getCurrentWebview }) => {
				const { label } = getCurrentWebview();
				return events.uiRequest.listen(({ payload }) => {
					if (payload.window === label && isDesktopUiAction(payload.action))
						drain();
				});
			})
			.then((unlisten) => {
				if (disposed) unlisten();
				else {
					stop = unlisten;
					drain();
				}
			})
			.catch((error) => logger.error(errorText(error)));
		return () => {
			disposed = true;
			stop?.();
		};
	}, []);
	return null;
}
