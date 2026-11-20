import { createStore } from "zustand/vanilla";

export type ChatGptQuote = { id: number; text: string; source?: string };

type ChatGptSidebarState = {
	requestId: number;
	pendingPrompt: string | null;
	quotes: ChatGptQuote[];
};

export const chatGptSidebarStore = createStore<ChatGptSidebarState>(() => ({
	requestId: 0,
	pendingPrompt: null,
	quotes: [],
}));

let nextQuoteId = 0;

export function addChatGptQuote(quote: Omit<ChatGptQuote, "id">): void {
	chatGptSidebarStore.setState(({ quotes }) => {
		if (
			quotes.some(
				(item) => item.text === quote.text && item.source === quote.source,
			)
		)
			return { quotes };
		return { quotes: [...quotes, { ...quote, id: ++nextQuoteId }] };
	});
}

export function removeChatGptQuote(id: number): void {
	chatGptSidebarStore.setState(({ quotes }) => ({
		quotes: quotes.filter((quote) => quote.id !== id),
	}));
}

export function requestChatGptSidebar(prompt?: string): number {
	const requestId = chatGptSidebarStore.getState().requestId + 1;
	chatGptSidebarStore.setState({
		requestId,
		pendingPrompt: prompt?.trim() || null,
	});
	return requestId;
}

export function consumeChatGptSidebarRequest(requestId: number): void {
	chatGptSidebarStore.setState((state) =>
		state.requestId === requestId ? { pendingPrompt: null } : state,
	);
}
