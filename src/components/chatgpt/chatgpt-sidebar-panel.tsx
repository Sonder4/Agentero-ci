import { Bot } from "lucide-react";
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
import { useChatGptSidebarStore } from "@/hooks/use-app-stores";
import { consumeChatGptSidebarRequest } from "@/lib/chatgpt/store";
import { commands } from "@/lib/core/bindings";
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
	const pendingPrompt = useChatGptSidebarStore((state) => state.pendingPrompt);
	const pendingPromptRef = useRef(pendingPrompt);
	pendingPromptRef.current = pendingPrompt;
	const slotRef = useRef<HTMLDivElement>(null);
	const frameRef = useRef<number | null>(null);
	const [bounds, setBounds] = useState<NativeBounds | null>(null);
	const openingRef = useRef(false);
	const handledRequestRef = useRef(0);

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
		if (!active) {
			void commands.chatgptSidebarHide();
			return;
		}
		if (!bounds || openingRef.current) return;
		const hasPendingRequest = requestId > handledRequestRef.current;
		const query = hasPendingRequest ? pendingPromptRef.current : null;
		openingRef.current = true;
		void commands
			.chatgptSidebarOpen(query, bounds)
			.then((result) => {
				if (result.status === "error") {
					notifyError(result.error);
					return;
				}
				if (hasPendingRequest) {
					handledRequestRef.current = requestId;
					consumeChatGptSidebarRequest(requestId);
				}
			})
			.catch((error) => notifyError(errorText(error)))
			.finally(() => {
				openingRef.current = false;
			});
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
