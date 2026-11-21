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
	"wait",
	"slide",
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
	"input[type=range]",
	TEXT_INPUT_SELECTOR,
	"summary",
	"[role=button]",
	"[role=checkbox]",
	"[role=switch]",
	"[role=radio]",
	"[role=slider]",
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

const IMPLICIT_ROLES: [string, string][] = [
	["input[type=range]", "slider"],
	["input[type=checkbox]", "checkbox"],
	["input[type=radio]", "radio"],
	["input[type=button], input[type=submit], button, summary", "button"],
	[`${TEXT_INPUT_SELECTOR}, textarea, [contenteditable=true]`, "textbox"],
	["select", "combobox"],
	["a[href]", "link"],
];

function controlRole(element: HTMLElement): string {
	return (
		element.getAttribute("role") ??
		IMPLICIT_ROLES.find(([selector]) => element.matches(selector))?.[1] ??
		element.localName
	);
}

type ControlState = Record<string, string | number | boolean>;

/** Observable state for scripts: checked / selected / expanded / value … */
function controlState(element: HTMLElement): ControlState | undefined {
	const state: ControlState = {};
	const aria = (name: string) => element.getAttribute(`aria-${name}`);
	if (
		element instanceof HTMLInputElement &&
		(element.type === "checkbox" || element.type === "radio")
	) {
		state.checked = element.indeterminate ? "mixed" : element.checked;
	} else if (aria("checked") != null) {
		state.checked =
			aria("checked") === "mixed" ? "mixed" : aria("checked") === "true";
	}
	for (const key of ["selected", "expanded", "pressed"]) {
		const value = aria(key);
		if (value === "true" || value === "false") state[key] = value === "true";
	}
	const slider = sliderRange(element);
	if (slider) Object.assign(state, slider);
	else if (
		element instanceof HTMLInputElement ||
		element instanceof HTMLTextAreaElement ||
		element instanceof HTMLSelectElement
	) {
		if (!element.matches("input[type=checkbox], input[type=radio]"))
			state.value = element.value.slice(0, 200);
	} else if (
		element.isContentEditable ||
		// Radix Select-style triggers show the chosen option as their text.
		element.getAttribute("role") === "combobox"
	) {
		state.value = element.innerText.trim().slice(0, 200);
	}
	if (document.activeElement === element) state.focused = true;
	return Object.keys(state).length ? state : undefined;
}

function sliderRange(element: HTMLElement) {
	if (element instanceof HTMLInputElement && element.type === "range") {
		const min = Number(element.min || 0);
		const max = Number(element.max || 100);
		const step = element.step === "any" ? 0 : Number(element.step || 1);
		return { value: element.valueAsNumber, min, max, step };
	}
	if (element.getAttribute("role") !== "slider") return undefined;
	const number = (name: string, fallback: number) => {
		const value = Number(element.getAttribute(`aria-value${name}`));
		return Number.isFinite(value) ? value : fallback;
	};
	return {
		value: number("now", Number.NaN),
		min: number("min", 0),
		max: number("max", 100),
	};
}

function describeState(state: ControlState | undefined): string {
	if (!state) return "";
	const parts = Object.entries(state).map(([key, value]) =>
		value === true
			? key
			: value === false
				? `not ${key}`
				: `${key}=${JSON.stringify(value)}`,
	);
	return ` (${parts.join(", ")})`;
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

/** Enabled, rendered controls matching an action id, else an exact label. */
function matchingControls(target: string) {
	const controls = availableControls();
	const byId = controls.filter((control) => control.id === target);
	return byId.length
		? byId
		: controls.filter((control) => control.label === target);
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Resolve one visible control like a click would, waiting up to 2 seconds. */
async function resolveControl(target: string, index: number | undefined) {
	const findControls = () => {
		const candidates = matchingControls(target);
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
		await sleep(125);
		matches = findControls();
	}
	if (matches.length > 1 && index == null)
		throw new Error(
			`Multiple controls match "${target}". Choose --occurrence from ui actions.`,
		);
	const control = matches[(index ?? 1) - 1];
	if (!control) throw new Error(`No enabled control matches "${target}".`);
	return control;
}

type WaitSpec = { target: string; gone?: boolean; timeoutMs: number };

async function waitForControl({ target, gone = false, timeoutMs }: WaitSpec) {
	const started = Date.now();
	const deadline = started + timeoutMs;
	for (;;) {
		const matches = matchingControls(target);
		if (gone ? !matches.length : matches.length) {
			const elapsedMs = Date.now() - started;
			return {
				action: "wait",
				target,
				gone,
				elapsedMs,
				controls: matches.map(({ id, label, element }) => ({
					id,
					label,
					role: controlRole(element),
					state: controlState(element),
					visible: Boolean(controlPoint(element)),
				})),
				lines: [
					gone
						? `"${target}" is gone after ${elapsedMs} ms`
						: `"${target}" is present after ${elapsedMs} ms`,
				],
			};
		}
		const left = deadline - Date.now();
		if (left <= 0)
			throw new Error(
				`Timed out after ${timeoutMs} ms waiting for "${target}" to ${gone ? "disappear" : "appear"}.`,
			);
		await sleep(Math.min(100, left));
	}
}

type SlideSpec = { target: string; occurrence?: number; value: string };

const nextFrame = () =>
	new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

async function slideControl({ target, occurrence, value: raw }: SlideSpec) {
	// Same parsing as the Host validation: trimmed, `+N` / `-N` = steps.
	const value = raw.trim();
	const { element } = await resolveControl(target, occurrence);
	const range = sliderRange(element);
	if (!range) throw new Error(`"${target}" is not a slider.`);
	const { min, max } = range;
	const before = range.value;
	const steps = /^[+-]\d+$/.test(value) ? Number(value) : undefined;
	element.focus();
	if (element instanceof HTMLInputElement) {
		const step = "step" in range && range.step ? range.step : 1;
		const next =
			value === "min"
				? min
				: value === "max"
					? max
					: steps !== undefined
						? before + steps * step
						: Number(value);
		if (next < min || next > max)
			throw new Error(`${next} is outside the slider range ${min}–${max}.`);
		// Native setter + input event: the same path as dragging the thumb.
		Object.getOwnPropertyDescriptor(
			HTMLInputElement.prototype,
			"value",
		)?.set?.call(element, String(next));
		element.dispatchEvent(new Event("input", { bubbles: true }));
		element.dispatchEvent(new Event("change", { bubbles: true }));
	} else {
		await slideWithKeys(element, value, steps, min, max);
	}
	await nextFrame();
	const after = sliderRange(element)?.value;
	return {
		action: "slide",
		target,
		before,
		value: after,
		min,
		max,
		lines: [`Slid "${target}" from ${before} to ${after} (${min}–${max})`],
	};
}

/** ARIA sliders (no native input): drive them with the keyboard contract. */
async function slideWithKeys(
	element: HTMLElement,
	value: string,
	steps: number | undefined,
	min: number,
	max: number,
) {
	const press = async (key: string) => {
		const init = { key, code: key, bubbles: true, cancelable: true };
		element.dispatchEvent(new KeyboardEvent("keydown", init));
		element.dispatchEvent(new KeyboardEvent("keyup", init));
		await nextFrame();
	};
	const current = () => sliderRange(element)?.value ?? Number.NaN;
	if (value === "min" || value === "max")
		return press(value === "min" ? "Home" : "End");
	if (steps !== undefined) {
		for (let i = 0; i < Math.abs(steps); i++)
			await press(steps > 0 ? "ArrowRight" : "ArrowLeft");
		return;
	}
	const goal = Number(value);
	if (goal < min || goal > max)
		throw new Error(`${goal} is outside the slider range ${min}–${max}.`);
	for (let i = 0; i < 1000 && current() !== goal; i++) {
		const was = current();
		const up = was < goal;
		await press(up ? "ArrowRight" : "ArrowLeft");
		const now = current();
		if (now === was)
			throw new Error("The slider did not respond to arrow keys.");
		// Stepped past the goal: keep whichever side is closer.
		if (up ? now > goal : now < goal) {
			if (Math.abs(now - goal) > Math.abs(was - goal))
				await press(up ? "ArrowLeft" : "ArrowRight");
			return;
		}
	}
}

async function clickControl(
	target: string,
	index: string | null | undefined,
	right: boolean,
) {
	const control = await resolveControl(
		target,
		index == null ? undefined : Number(index),
	);
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
			const controls = visibleControls().map(
				({ id, label, occurrence, element }) => ({
					id,
					label,
					occurrence,
					role: controlRole(element),
					state: controlState(element),
				}),
			);
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
						({ id, label, occurrence, role, state }) =>
							`${role === "slider" ? "slide " : "click "}   ${id ? `${id} · ` : ""}${label} [${occurrence}]${describeState(state)}`,
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
		case "wait":
			return waitForControl(JSON.parse(target) as WaitSpec);
		case "slide":
			return slideControl(JSON.parse(target) as SlideSpec);
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
						// `id` matches the CLI's `actionId`, so a script can find
						// everything logged while its action ran.
						const fields = {
							id: request.reference,
							action: request.action,
							window: request.window,
							// Fill values may be secrets; never log them.
							target: request.action === "fill" ? undefined : request.path,
						};
						const started = performance.now();
						logger.info("op start ui_action", fields);
						const result = await runDesktopAction(request).catch((error) => ({
							error: errorText(error),
						}));
						const end = `op end ui_action ok=${!("error" in result)} duration_ms=${Math.round(performance.now() - started)}`;
						if ("error" in result)
							logger.warn(end, { ...fields, error: result.error });
						else logger.info(end, fields);
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
