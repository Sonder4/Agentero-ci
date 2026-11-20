/** Stable mapping between shared preference names and legacy WebView keys. */
export const PREFERENCE_STORAGE_KEYS = {
	texProjects: "agentero.texProjects.v1",
	texEngines: "agentero.texEngineByRoot",
	writingGithub: "agentero.writing.github.v1",
	writingSync: "agentero.writing.syncMode.v1",
	agentModels: "agentero-agent-model-pref",
	agentCollaboration: "agentero-agent-collaboration-pref",
	agentFavorites: "agentero-agent-model-favorites",
	agentReasoning: "agentero-agent-reasoning-effort-pref",
	zoteroImport: "motif.zotero.opts",
	zoteroSync: "motif.zotero.sync.opts",
	arxivCategories: "plaza:arxiv-rec:categories",
	textEditorFontSize: "agentero-text-editor-font-size",
	pdfPaperTone: "agentero-pdf-color-scheme",
	pdfMarkup: "agentero-pdf-markup-shapes",
	pdfCardWidths: "agentero-pdf-card-widths",
	texReviewWidth: "agentero-tex-review-width",
} as const;

export type FrontendPreferenceKey = keyof typeof PREFERENCE_STORAGE_KEYS;
