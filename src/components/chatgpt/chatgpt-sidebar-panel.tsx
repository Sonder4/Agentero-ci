import { Bot, ClipboardCopy, X } from "lucide-react";
import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { useTranslation } from "react-i18next";
import { ChatGPTIcon } from "@/components/agent/open-in-chat";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useChatGptSidebarStore } from "@/hooks/use-app-stores";
import {
	chatGptSidebarStore,
	consumeChatGptSidebarRequest,
	removeChatGptQuote,
} from "@/lib/chatgpt/store";
import { chatGptQuoteDraft } from "@/lib/chatgpt/window";
import { commands } from "@/lib/core/bindings";
import { copyTextToClipboard } from "@/lib/core/clipboard";
import { errorText } from "@/lib/core/error";
import { notifyError } from "@/lib/core/notify";

type NativeBounds = {
	x: number;
	y: number;
	width: number;
	height: number;
};

function boundsFor(element: HTMLElement): NativeBounds | null {
	const rect = element.getBoundingClientRect();
	if (rect.width < 1 || rect.height < 1) return null;
	return {
		x: Math.max(0, Math.round(rect.left)),
		y: Math.max(0, Math.round(rect.top)),
		width: Math.max(1, Math.round(rect.width)),
		height: Math.max(1, Math.round(rect.height)),
	};
}

function sameBounds(left: NativeBounds | null, right: NativeBounds | null) {
	return (
		left === right ||
		(left != null &&
			right != null &&
			left.x === right.x &&
			left.y === right.y &&
			left.width === right.width &&
			left.height === right.height)
	);
}

export function ChatGptSidebarPanel({
	active,
	onOpenAgent,
}: {
	active: boolean;
	onOpenAgent: () => void;
}) {
	const { t } = useTranslation("app");
	const requestId = useChatGptSidebarStore((state) => state.requestId);
	const quotes = useChatGptSidebarStore((state) => state.quotes);
	const question = useChatGptSidebarStore((state) => state.question);
	const slotRef = useRef<HTMLDivElement>(null);
	const frameRef = useRef<number | null>(null);
	const [bounds, setBounds] = useState<NativeBounds | null>(null);
	const pendingUpdate = useRef(Promise.resolve());
	const handledRequestRef = useRef(0);
	const [inserting, setInserting] = useState(false);

	const insertDraft = async () => {
		setInserting(true);
		try {
			await pendingUpdate.current;
			const result = await commands.chatgptSidebarInsertDraft(
				chatGptQuoteDraft(quotes, question),
			);
			if (result.status === "error") {
				notifyError(
					result.error === "composer_occupied"
						? t("chatgptQuote.draftOccupied")
						: t("chatgptQuote.insertFailed"),
				);
			}
		} catch (error) {
			notifyError(errorText(error));
		} finally {
			setInserting(false);
		}
	};

	const copyDraft = async () => {
		if (!(await copyTextToClipboard(chatGptQuoteDraft(quotes, question)))) {
			notifyError(t("chatgptQuote.copyFailed"));
		}
	};

	const measure = useCallback(() => {
		const slot = slotRef.current;
		if (!slot) return;
		const next = boundsFor(slot);
		setBounds((current) => (sameBounds(current, next) ? current : next));
	}, []);

	const scheduleMeasure = useCallback(() => {
		if (frameRef.current != null) {
			cancelAnimationFrame(frameRef.current);
		}
		frameRef.current = requestAnimationFrame(() => {
			frameRef.current = null;
			measure();
		});
	}, [measure]);

	useLayoutEffect(() => {
		if (!active) return;
		measure();
		const slot = slotRef.current;
		if (!slot) return;
		const observer = new ResizeObserver(scheduleMeasure);
		observer.observe(slot);
		window.addEventListener("resize", scheduleMeasure);
		return () => {
			observer.disconnect();
			window.removeEventListener("resize", scheduleMeasure);
			if (frameRef.current != null) {
				cancelAnimationFrame(frameRef.current);
				frameRef.current = null;
			}
		};
	}, [active, measure, scheduleMeasure]);

	useEffect(() => {
		let cancelled = false;
		pendingUpdate.current = pendingUpdate.current
			.then(async () => {
				if (cancelled) return;
				if (!active) {
					await commands.chatgptSidebarHide();
					return;
				}
				if (!bounds) return;
				const hasRequest = requestId > handledRequestRef.current;
				const query = hasRequest
					? chatGptSidebarStore.getState().pendingPrompt
					: null;
				const result = await commands.chatgptSidebarOpen(query, bounds);
				if (result.status === "error") {
					notifyError(result.error);
					return;
				}
				if (hasRequest) {
					handledRequestRef.current = requestId;
					consumeChatGptSidebarRequest(requestId);
				}
			})
			.catch((error) => {
				notifyError(errorText(error));
			});
		return () => {
			cancelled = true;
		};
	}, [active, bounds, requestId]);

	useEffect(() => {
		const close = () => {
			void commands.chatgptSidebarClose();
		};
		window.addEventListener("pagehide", close);
		return () => window.removeEventListener("pagehide", close);
	}, []);

	return (
		<div className="flex h-full min-h-0 flex-col bg-background">
			<div className="flex h-10 shrink-0 items-center gap-2 border-b px-3">
				<ChatGPTIcon className="size-4 shrink-0" />
				<span className="min-w-0 flex-1 truncate font-medium text-sm">
					ChatGPT
				</span>
				<Button
					type="button"
					variant="ghost"
					size="icon-sm"
					aria-label={t("labels.agent")}
					title={t("labels.agent")}
					onClick={onOpenAgent}
				>
					<Bot className="size-4" />
				</Button>
			</div>
			{quotes.length > 0 && (
				<section
					className="shrink-0 space-y-2 border-b p-3"
					aria-label={t("chatgptQuote.selection", { count: quotes.length })}
				>
					<div className="text-xs text-muted-foreground">
						{t("chatgptQuote.selection", { count: quotes.length })}
					</div>
					<div className="max-h-28 space-y-1 overflow-auto">
						{quotes.map((quote) => (
							<div
								key={quote.id}
								className="flex items-start gap-1 rounded-md bg-muted px-2 py-1"
							>
								<details className="min-w-0 flex-1 text-xs">
									<summary
										className="cursor-pointer truncate"
										title={quote.source}
									>
										{quote.text}
									</summary>
									{quote.source && (
										<div className="mt-1 break-all text-muted-foreground">
											{quote.source}
										</div>
									)}
									<blockquote className="mt-1 wrap-anywhere whitespace-pre-wrap">
										{quote.text}
									</blockquote>
								</details>
								<Button
									type="button"
									variant="ghost"
									size="icon-xs"
									aria-label={t("chatgptQuote.remove")}
									title={t("chatgptQuote.remove")}
									onClick={() => removeChatGptQuote(quote.id)}
								>
									<X className="size-3" />
								</Button>
							</div>
						))}
					</div>
					<Textarea
						data-ui-action="chatgpt.quote.question"
						className="min-h-12 resize-y"
						rows={2}
						aria-label={t("chatgptQuote.question")}
						placeholder={t("chatgptQuote.question")}
						value={question}
						onChange={(event) =>
							chatGptSidebarStore.setState({ question: event.target.value })
						}
					/>
					<div className="flex justify-end gap-1">
						<Button
							type="button"
							variant="ghost"
							size="icon-sm"
							data-ui-action="chatgpt.quote.copy"
							aria-label={t("chatgptQuote.copy")}
							title={t("chatgptQuote.copy")}
							onClick={() => void copyDraft()}
						>
							<ClipboardCopy className="size-4" />
						</Button>
						<Button
							type="button"
							size="sm"
							data-ui-action="chatgpt.quote.insert"
							title={t("chatgptQuote.insertHint")}
							disabled={inserting || !bounds}
							onClick={() => void insertDraft()}
						>
							{t("chatgptQuote.insert")}
						</Button>
					</div>
				</section>
			)}
			<div
				ref={slotRef}
				data-chatgpt-native-slot
				className="relative min-h-0 flex-1 overflow-hidden bg-white"
			>
				<div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-3 text-muted-foreground">
					<ChatGPTIcon className="size-8 opacity-30" />
					<span className="text-xs">ChatGPT</span>
				</div>
			</div>
		</div>
	);
}
