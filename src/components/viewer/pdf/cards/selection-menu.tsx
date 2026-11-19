import { Bookmark, MessageSquare } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChatGPTIcon } from "@/components/agent/open-in-chat";
import {
	SelectionActionMenu,
	type SelectionMenuMoreAction,
	type SelectionMenuShortcut,
} from "@/components/selection/selection-action-menu";
import {
	SelectionToolbarAction,
	SelectionToolbarDivider,
	SelectionToolbarIconButton,
} from "@/components/selection/selection-toolbar";
import type { ScreenPoint } from "@/components/viewer/pdf/types";
import {
	type ChatGptSelectionSource,
	chatGptPromptForText,
	openChatGptSidebar,
} from "@/lib/chatgpt/window";
import {
	type MarkupShapes,
	oppositeTextMarkupKind,
	readLastMarkupShape,
	readMarkupShapes,
	writeLastMarkupShape,
	writeMarkupShape,
} from "@/lib/pdf/highlight/markup-pref";
import {
	HIGHLIGHT_COLORS,
	type HighlightColor,
} from "@/lib/pdf/highlight/palette";
import type { PdfTextMarkupKind } from "@/lib/pdf/highlight/types";
import {
	activateMenuContribution,
	type MenuContext,
	visibleMenuContributions,
} from "@/lib/shell/commands/menu-contributions";
import {
	HIGHLIGHT_COLOR_STACK_COLLAPSED_W,
	HIGHLIGHT_COLOR_STACK_EXPANDED_W,
	HighlightColorStack,
} from "./highlight-color-stack";

type SelectionMenuProps = {
	screen: ScreenPoint;
	onHighlight?: (color: HighlightColor, shape: PdfTextMarkupKind) => void;
	onAsk?: () => void;
	onAddToChat?: () => void;
	onTranslate?: () => void;
	selectionText?: string;
	onCopy?: () => Promise<boolean>;
	onBookmark?: () => void;
	onNote?: (shape: PdfTextMarkupKind) => void;
	showHighlight?: boolean;
	showTranslate?: boolean;
	contributionContext?: MenuContext;
	source?: ChatGptSelectionSource;
};

/** PDF-only extensions composed into the application selection menu. */
export function SelectionMenu({
	screen,
	onHighlight,
	onAsk,
	onAddToChat,
	onTranslate,
	selectionText,
	onCopy,
	onBookmark,
	onNote,
	showHighlight = true,
	showTranslate = true,
	contributionContext,
	source,
}: SelectionMenuProps) {
	const { t } = useTranslation("viewer");
	const dividerRef = useRef<HTMLDivElement>(null);
	const colorsRef = useRef<HTMLFieldSetElement>(null);
	const [shapes, setShapes] = useState<MarkupShapes>(readMarkupShapes);
	// Notes take no color, so they follow the last shape the user touched.
	const [noteShape, setNoteShape] =
		useState<PdfTextMarkupKind>(readLastMarkupShape);
	const toggleShape = useCallback(
		(color: HighlightColor) => {
			const next = oppositeTextMarkupKind(shapes[color]);
			writeMarkupShape(color, next);
			setShapes({ ...shapes, [color]: next });
			setNoteShape(next);
		},
		[shapes],
	);
	const applyShape = useCallback(
		(color: HighlightColor) => {
			const shape = shapes[color];
			writeMarkupShape(color, shape);
			setNoteShape(shape);
			onHighlight?.(color, shape);
		},
		[shapes, onHighlight],
	);
	/**
	 * Forced shape from a modifier shortcut: applies without flipping the
	 * swatch's remembered click shape.
	 */
	const applyForcedShape = useCallback(
		(color: HighlightColor, kind: PdfTextMarkupKind) => {
			writeLastMarkupShape(kind);
			setNoteShape(kind);
			onHighlight?.(color, kind);
		},
		[onHighlight],
	);
	// Bare digits come from the palette's aria hints. Each modifier prefix
	// forces one shape; a future wavy shape adds its own prefix here.
	const shortcutActions = useMemo<SelectionMenuShortcut[]>(
		() =>
			HIGHLIGHT_COLORS.map((color, i) => ({
				key: String(i + 1),
				alt: true,
				run: () => applyForcedShape(color, "underline"),
			})),
		[applyForcedShape],
	);
	const contributions = contributionContext
		? visibleMenuContributions("pdf.selection", contributionContext)
		: [];
	const showPalette = showHighlight && Boolean(onHighlight);
	const moreActions: SelectionMenuMoreAction[] = [];
	if (onBookmark) {
		moreActions.push({
			id: "bookmark",
			label: t("selection.bookmark"),
			icon: <Bookmark className="size-3.5" />,
			shortcut: "B",
			onSelect: onBookmark,
		});
	}
	if (selectionText?.trim()) {
		moreActions.push({
			id: "chatgpt",
			label: t("selection.openInChatGpt"),
			icon: <ChatGPTIcon className="size-3.5" />,
			onSelect: () => {
				void openChatGptSidebar(chatGptPromptForText(selectionText, source));
			},
		});
	}
	return (
		<SelectionActionMenu
			screen={screen}
			selectionText={selectionText}
			onAsk={onAsk}
			onAddToChat={onAddToChat}
			onTranslate={showTranslate ? onTranslate : undefined}
			onCopy={onCopy}
			anchorRef={dividerRef}
			expandingRef={colorsRef}
			expandedWidth={HIGHLIGHT_COLOR_STACK_EXPANDED_W}
			collapsedWidth={HIGHLIGHT_COLOR_STACK_COLLAPSED_W}
			shortcutActions={showPalette ? shortcutActions : undefined}
			beforeActions={
				showHighlight && (showPalette || onNote) ? (
					<>
						{showPalette && (
							<HighlightColorStack
								ref={colorsRef}
								shapes={shapes}
								onToggleShape={toggleShape}
								onSelect={applyShape}
								shortcuts
							/>
						)}
						{onNote && (
							<SelectionToolbarIconButton
								label={t("selection.note")}
								shortcut="N"
								onClick={() => onNote(noteShape)}
							>
								<MessageSquare className="size-3.5" />
							</SelectionToolbarIconButton>
						)}
						{(onCopy || (showTranslate && onTranslate)) && (
							<SelectionToolbarDivider
								ref={dividerRef}
								data-selection-anchor=""
							/>
						)}
					</>
				) : undefined
			}
			moreActions={moreActions.length > 0 ? moreActions : undefined}
			afterActions={
				contributions.length > 0 ? (
					<>
						<SelectionToolbarDivider />
						{contributions.map((item) => (
							<SelectionToolbarAction
								key={item.id}
								label={item.title}
								onClick={() =>
									activateMenuContribution(item, contributionContext)
								}
							/>
						))}
					</>
				) : undefined
			}
		/>
	);
}
