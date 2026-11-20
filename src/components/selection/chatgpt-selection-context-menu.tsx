import { Copy } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useStore } from "zustand";
import { ChatGPTIcon } from "@/components/agent/open-in-chat";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { chatGptSelectionMenu } from "@/lib/chatgpt/selection-menu";
import { openChatGptQuote } from "@/lib/chatgpt/window";
import { copyTextToClipboard } from "@/lib/core/clipboard";
import { notifyError } from "@/lib/core/notify";

export function ChatGptSelectionContextMenu() {
	const { t } = useTranslation("viewer");
	const request = useStore(chatGptSelectionMenu, (state) => state.request);
	return (
		<DropdownMenu
			open={request != null}
			modal={false}
			onOpenChange={(open) => {
				if (!open) chatGptSelectionMenu.setState({ request: null });
			}}
		>
			<DropdownMenuTrigger asChild>
				<button
					type="button"
					tabIndex={-1}
					aria-hidden
					className="pointer-events-none fixed size-px opacity-0"
					style={{ left: request?.x, top: request?.y }}
				/>
			</DropdownMenuTrigger>
			{request && (
				<DropdownMenuContent
					data-selection-ui
					sideOffset={0}
					className="w-auto"
					onCloseAutoFocus={(event) => event.preventDefault()}
				>
					<DropdownMenuItem
						data-ui-action="chatgpt.selection.ask"
						onSelect={() => void openChatGptQuote(request.text, request.source)}
					>
						<ChatGPTIcon />
						{t("selection.openInChatGpt")}
					</DropdownMenuItem>
					<DropdownMenuItem
						onSelect={() => {
							void copyTextToClipboard(request.text).then((copied) => {
								if (!copied) notifyError(t("selection.copyFailed"));
							});
						}}
					>
						<Copy />
						{t("selection.copy")}
					</DropdownMenuItem>
				</DropdownMenuContent>
			)}
		</DropdownMenu>
	);
}
