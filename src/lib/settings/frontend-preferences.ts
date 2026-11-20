/** Bridge selected persistent WebView preferences to the shared settings file. */
import { commands, type Json } from "@/lib/core/bindings";
import { callApi } from "@/lib/core/ipc";
import { PREFERENCE_STORAGE_KEYS } from "@/lib/core/preference-keys";
import { observeJsonStorageWrites } from "@/lib/core/storage";
import { isTauri } from "@/lib/core/tauri";
import {
	applyExternalSettings,
	loadSettings,
	subscribeSettings,
} from "@/lib/settings/store";

const MIGRATED_KEY = ["agentero", "frontendPreferencesMigrated", "v1"].join(
	".",
);
const aliases = Object.entries(PREFERENCE_STORAGE_KEYS);
const aliasByStorageKey = new Map<string, string>(
	aliases.map(([alias, key]) => [key, alias]),
);
const pending = new Map<string, number>();
const queues = new Map<string, Promise<void>>();

function readStored(key: string): Json | null {
	const raw = localStorage.getItem(key);
	if (raw === null) return null;
	try {
		return JSON.parse(raw) as Json;
	} catch {
		return null;
	}
}

function applyPreferences(preferences: Record<string, unknown>): void {
	for (const [alias, key] of aliases) {
		if (pending.get(alias)) continue;
		const present = Object.hasOwn(preferences, alias);
		const next = present ? JSON.stringify(preferences[alias]) : null;
		if (localStorage.getItem(key) === next) continue;
		if (next === null || next === undefined) localStorage.removeItem(key);
		else localStorage.setItem(key, next);
		window.dispatchEvent(
			new CustomEvent("agentero:frontend-preference-changed", {
				detail: { key },
			}),
		);
	}
}

function snapshot(value: unknown): Json | null {
	if (value === null || value === undefined) return null;
	return JSON.parse(JSON.stringify(value)) as Json;
}

/** Called once after the Host settings cache is loaded, before mounting UI. */
export async function initFrontendPreferences(
	onError: (error: unknown) => void,
): Promise<void> {
	if (!isTauri()) return;
	try {
		// Fetch an authoritative snapshot before touching any legacy storage.
		const current = await callApi(() => commands.settingsGet(), {
			fallback: "settings_get failed",
		});
		applyExternalSettings(current.settings as ReturnType<typeof loadSettings>);
		if (localStorage.getItem(MIGRATED_KEY) !== "1") {
			const updates = aliases.flatMap(([alias, key]) => {
				const value = readStored(key);
				return value === null
					? []
					: [{ key: alias, value, expected: null, onlyIfMissing: true }];
			});
			if (updates.length) {
				const migrated = await callApi(
					() => commands.settingsUpdateFrontendPreferences(updates),
					{ fallback: "frontend preference migration failed" },
				);
				applyExternalSettings(migrated as ReturnType<typeof loadSettings>);
			}
			localStorage.setItem(MIGRATED_KEY, "1");
		}
		applyPreferences(loadSettings().frontendPreferences);
		subscribeSettings((settings) =>
			applyPreferences(settings.frontendPreferences),
		);
		observeJsonStorageWrites((storageKey, before, after) => {
			const alias = aliasByStorageKey.get(storageKey);
			if (!alias) return;
			const expected = snapshot(before);
			const value = snapshot(after);
			pending.set(alias, (pending.get(alias) ?? 0) + 1);
			const previous = queues.get(alias) ?? Promise.resolve();
			const task = previous
				.catch(() => undefined)
				.then(async () => {
					const saved = await callApi(
						() =>
							commands.settingsUpdateFrontendPreferences([
								{ key: alias, value, expected, onlyIfMissing: false },
							]),
						{ fallback: "frontend preference save failed" },
					);
					applyExternalSettings(saved as ReturnType<typeof loadSettings>);
				})
				.catch(async (error) => {
					onError(error);
					try {
						const fresh = await callApi(() => commands.settingsGet(), {
							fallback: "settings_get failed",
						});
						applyExternalSettings(
							fresh.settings as ReturnType<typeof loadSettings>,
						);
					} catch (reloadError) {
						console.warn("[preferences] reload failed", reloadError);
					}
				})
				.finally(() => {
					const remaining = (pending.get(alias) ?? 1) - 1;
					if (remaining) pending.set(alias, remaining);
					else {
						pending.delete(alias);
						applyPreferences(loadSettings().frontendPreferences);
					}
				});
			queues.set(alias, task);
		});
	} catch (error) {
		// Keep the original WebView data intact if migration or Host access fails.
		onError(error);
	}
}
