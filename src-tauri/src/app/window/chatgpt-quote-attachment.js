(function attachQuote({ url, filename, preview, text }) {
	if (location.origin !== "https://chatgpt.com") return "wrong_origin";
	if (location.href !== url) return "conversation_changed";

	const editor = Array.from(
		document.querySelectorAll(
			"#prompt-textarea, form[data-chatgpt-composer] [contenteditable=true][role=textbox]",
		),
	).find(
		(element) =>
			!element.closest("[inert], [aria-hidden=true], [role=dialog]") &&
			element.getClientRects().length > 0 &&
			getComputedStyle(element).visibility === "visible",
	);
	const form = editor?.closest("form");
	if (!form) return "composer_missing";

	// The file label is rendered by ChatGPT. Search only the active composer
	// and ignore the editor, which may already contain the user's question.
	const normalizedName = filename.normalize("NFC");
	const isFileName = (value) => {
		const name = value?.trim().normalize("NFC");
		if (name === normalizedName) return true;
		// ChatGPT can add a numeric suffix when a file with the same name
		// already exists. Match the complete stem, including the quote hash.
		return name?.replace(/\(\d+\)(?=\.md$)/, "") === normalizedName;
	};
	const label = Array.from(
		form.querySelectorAll('[title], span, div, button[aria-label]'),
	).find(
		(element) =>
			!element.contains(editor) &&
			!editor.contains(element) &&
			(isFileName(element.title) ||
				isFileName(element.getAttribute("aria-label")) ||
				(element.childElementCount === 0 && isFileName(element.textContent))),
	);
	if (label) {
		const card = label.closest("span.composer-attachment-surface");
		const thumbnail = card?.firstElementChild;
		if (card?.querySelector('[role="alert"]')) return "attachment_failed";
		if (
			Array.from(thumbnail?.querySelectorAll('[role="progressbar"], [role="status"]') ?? [])
				.some((indicator) => {
					const bounds = indicator.getBoundingClientRect();
					return bounds.width > 2 && bounds.height > 2;
				})
		) {
			return "processing";
		}
		// Augment the native thumbnail when its layout is available. Keep the
		// file title, upload status, open button and remove button owned by ChatGPT.
		if (
			preview &&
			thumbnail?.tagName === "SPAN" &&
			!thumbnail.contains(label) &&
			!thumbnail.querySelector("[data-agentero-quote-preview]")
		) {
			const excerpt = document.createElement("span");
			excerpt.setAttribute("data-agentero-quote-preview", "");
			excerpt.textContent = preview;
			Object.assign(excerpt.style, {
				position: "absolute",
				inset: "0",
				padding: "0.5rem",
				pointerEvents: "none",
				overflow: "hidden",
				overflowWrap: "anywhere",
				whiteSpace: "pre-line",
				fontSize: "0.8125rem",
				lineHeight: "1.35",
				backgroundColor: getComputedStyle(card).backgroundColor,
			});
			thumbnail.style.position = "relative";
			thumbnail.append(excerpt);
		}
		return "attached";
	}
	if (text === undefined) return "waiting";

	// The general picker accepts documents. Other inputs belong to image,
	// camera or audio controls and must never receive the excerpt.
	const pickers = form.querySelectorAll(
		'input[type=file][multiple]:not([accept]):not(:disabled), input[type=file][multiple][accept=""]:not(:disabled)',
	);
	const input = pickers.length === 1 ? pickers[0] : null;
	if (!input) return "attachment_unavailable";

	const transfer = new DataTransfer();
	transfer.items.add(new File([text], filename, { type: "text/markdown" }));
	input.files = transfer.files;
	input.dispatchEvent(new Event("change", { bubbles: true }));
	return "queued";
})
