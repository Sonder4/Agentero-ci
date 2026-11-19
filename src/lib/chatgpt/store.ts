import { createStore } from "zustand/vanilla";

type ChatGptSidebarState = {
	requestId: number;
	pendingPrompt: string | null;
};

export const chatGptSidebarStore = createStore<ChatGptSidebarState>(() => ({
	requestId: 0,
	pendingPrompt: null,
}));

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
