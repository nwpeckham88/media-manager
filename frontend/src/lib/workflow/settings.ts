import { writable } from 'svelte/store';
import type { HashingMode, RenamePreset } from '$lib/workflow/onboarding';
import { isBrowser } from '$lib/utils/browser';

export type DashboardRefreshPolicy = 'running-jobs-only' | 'always' | 'manual';

export type AppSettings = {
	defaultHashingMode: HashingMode;
	renamePreset: RenamePreset;
	dashboardRefreshPolicy: DashboardRefreshPolicy;
};

const STORAGE_KEY = 'mm-app-settings-v1';

const DEFAULT_SETTINGS: AppSettings = {
	defaultHashingMode: 'hybrid',
	renamePreset: 'movie_year',
	dashboardRefreshPolicy: 'running-jobs-only'
};

function normalizeSettings(value: unknown): AppSettings {
	if (!value || typeof value !== 'object') {
		return { ...DEFAULT_SETTINGS };
	}

	const source = value as Partial<Record<keyof AppSettings, unknown>>;
	return {
		defaultHashingMode: source.defaultHashingMode === 'strict' ? 'strict' : 'hybrid',
		renamePreset: 'movie_year',
		dashboardRefreshPolicy:
			source.dashboardRefreshPolicy === 'always' || source.dashboardRefreshPolicy === 'manual'
				? source.dashboardRefreshPolicy
				: 'running-jobs-only'
	};
}

function readStoredSettings(): AppSettings {
	if (!isBrowser()) {
		return { ...DEFAULT_SETTINGS };
	}

	const raw = window.localStorage.getItem(STORAGE_KEY);
	if (!raw) {
		return { ...DEFAULT_SETTINGS };
	}

	try {
		return normalizeSettings(JSON.parse(raw));
	} catch {
		return { ...DEFAULT_SETTINGS };
	}
}

function persistSettings(value: AppSettings): void {
	if (!isBrowser()) {
		return;
	}

	window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
}

export const appSettings = writable<AppSettings>(readStoredSettings());

if (isBrowser()) {
	appSettings.subscribe((value) => {
		persistSettings(value);
	});
}

export function updateAppSettings(update: Partial<AppSettings>): void {
	appSettings.update((current) => normalizeSettings({ ...current, ...update }));
}

export function getDefaultSettings(): AppSettings {
	return { ...DEFAULT_SETTINGS };
}
