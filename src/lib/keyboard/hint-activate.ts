/**
 * Activate a hint target the way a pointer user would.
 *
 * `element.click()` only fires the `click` event. Triggers that open on
 * `pointerdown` — Radix / Base UI menus and selects such as the Agent
 * switcher or the title-bar settings gear — only received focus, so the
 * dropdown never opened. Dispatch the full pointer → mouse → click sequence
 * instead: click-only triggers still work, pointerdown triggers now open.
 */
export function activateHintTarget(
	element: HTMLElement,
	point?: { x: number; y: number },
): void {
	element.focus({ preventScroll: true });
	// Radix Select swallows the first pointerup after its trigger's
	// pointerdown and only selects mouse items on pointerup. A synthetic
	// sequence opens the menu in the same tick, so the option's pointerup is
	// the one swallowed. Use the item's keyboard contract (Enter) instead.
	if (
		element.matches("[role=option]") &&
		element.closest("[data-radix-select-viewport]")
	) {
		const key = {
			key: "Enter",
			code: "Enter",
			bubbles: true,
			cancelable: true,
		};
		element.dispatchEvent(new KeyboardEvent("keydown", key));
		element.dispatchEvent(new KeyboardEvent("keyup", key));
		return;
	}
	const rect = element.getBoundingClientRect();
	const x = point?.x ?? rect.left + rect.width / 2;
	const y = point?.y ?? rect.top + rect.height / 2;
	const base = {
		bubbles: true,
		cancelable: true,
		composed: true,
		view: window,
		clientX: x,
		clientY: y,
		screenX: x,
		screenY: y,
		button: 0,
	};
	const hasPointer = typeof PointerEvent === "function";
	const pointer = (type: string, buttons: number) =>
		new PointerEvent(type, {
			...base,
			buttons,
			pointerId: 1,
			pointerType: "mouse",
			isPrimary: true,
		});
	const mouse = (type: string, buttons: number) =>
		new MouseEvent(type, {
			...base,
			buttons,
			detail: type === "click" ? 1 : 0,
		});

	if (hasPointer) element.dispatchEvent(pointer("pointerdown", 1));
	element.dispatchEvent(mouse("mousedown", 1));
	if (hasPointer) element.dispatchEvent(pointer("pointerup", 0));
	element.dispatchEvent(mouse("mouseup", 0));
	element.dispatchEvent(mouse("click", 0));
}
