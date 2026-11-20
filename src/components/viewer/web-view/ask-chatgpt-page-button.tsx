import { useTranslation } from "react-i18next";
import { ChatGPTIcon } from "@/components/agent/open-in-chat";
import { Button } from "@/components/ui/button";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";

export function AskChatGptPageButton({ onAsk }: { onAsk: () => void }) {
	const { t } = useTranslation("viewer");
	const label = t("selection.askChatGptAboutPage");
	return (
		<Tooltip>
			<TooltipTrigger asChild>
				<Button
					type="button"
					variant="ghost"
					size="icon-xs"
					aria-label={label}
					data-ui-action="chatgpt.page.ask"
					onClick={onAsk}
				>
					<ChatGPTIcon className="size-3.5" />
				</Button>
			</TooltipTrigger>
			<TooltipContent side="bottom">{label}</TooltipContent>
		</Tooltip>
	);
}
