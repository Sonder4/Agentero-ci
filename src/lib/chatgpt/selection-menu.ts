import { createStore } from "zustand/vanilla";
import type { ChatGptSelectionSource } from "@/lib/chatgpt/window";

type SelectionMenuRequest = {
	x: number;
	y: number;
	text: string;
	source?: ChatGptSelectionSource;
	askPage?: () => void;
};

export const chatGptSelectionMenu = createStore<{
	request: SelectionMenuRequest | null;
}>(() => ({ request: null }));

export function openChatGptSelectionMenu(request: SelectionMenuRequest): void {
	if (request.text.trim() || request.askPage)
		chatGptSelectionMenu.setState({ request });
}
