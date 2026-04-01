import { writable } from 'svelte/store';
import { isBrowser } from '$lib/utils/browser';

export type OnboardingStep = 1 | 2 | 3 | 4;
export type HashingMode = 'hybrid' | 'strict';
export type RenamePreset = 'movie_year';

export type OnboardingState = {
	step: OnboardingStep;
	hashingMode: HashingMode;
	renamePreset: RenamePreset;
	lastDetectedRoots: number;
	lastDetectedMediaFiles: number;
	completedAt: string | null;
};

export const ONBOARDING_STORAGE_KEY = 'mm-onboarding-v1';
export const ONBOARDING_COMPLETE_KEY = 'mm-onboarding-complete-v1';

const DEFAULT_ONBOARDING_STATE: OnboardingState = {
	step: 1,
	hashingMode: 'hybrid',
	renamePreset: 'movie_year',
	lastDetectedRoots: 0,
	lastDetectedMediaFiles: 0,
	completedAt: null
};

function parseStep(value: unknown): OnboardingStep {
	if (value === 2 || value === 3 || value === 4) {
		return value;
	}
	return 1;
}

function normalizeOnboardingState(value: unknown): OnboardingState {
	if (!value || typeof value !== 'object') {
		return { ...DEFAULT_ONBOARDING_STATE };
	}

	const source = value as Partial<Record<keyof OnboardingState, unknown>>;
	const mode = source.hashingMode === 'strict' ? 'strict' : 'hybrid';
	const renamePreset = source.renamePreset === 'movie_year' ? 'movie_year' : 'movie_year';
	const completedAt = typeof source.completedAt === 'string' ? source.completedAt : null;

	return {
		step: parseStep(source.step),
		hashingMode: mode,
		renamePreset: renamePreset,
		lastDetectedRoots: Number.isFinite(source.lastDetectedRoots) ? Number(source.lastDetectedRoots) : 0,
		lastDetectedMediaFiles: Number.isFinite(source.lastDetectedMediaFiles)
			? Number(source.lastDetectedMediaFiles)
			: 0,
		completedAt
	};
}

function readStoredState(): OnboardingState {
	if (!isBrowser()) {
		return { ...DEFAULT_ONBOARDING_STATE };
	}

	const raw = window.localStorage.getItem(ONBOARDING_STORAGE_KEY);
	if (!raw) {
		return { ...DEFAULT_ONBOARDING_STATE };
	}

	try {
		return normalizeOnboardingState(JSON.parse(raw));
	} catch {
		return { ...DEFAULT_ONBOARDING_STATE };
	}
}

function persistState(value: OnboardingState): void {
	if (!isBrowser()) {
		return;
	}

	window.localStorage.setItem(ONBOARDING_STORAGE_KEY, JSON.stringify(value));
}

export const onboardingState = writable<OnboardingState>(readStoredState());

if (isBrowser()) {
	onboardingState.subscribe((value) => {
		persistState(value);
	});
}

export function updateOnboardingState(update: Partial<OnboardingState>): void {
	onboardingState.update((current) => ({
		...current,
		...normalizeOnboardingState({ ...current, ...update })
	}));
}

export function completeOnboarding(): void {
	const completedAt = new Date().toISOString();
	if (isBrowser()) {
		window.localStorage.setItem(ONBOARDING_COMPLETE_KEY, 'true');
	}
	updateOnboardingState({ completedAt, step: 4 });
}

export function resetOnboardingState(): void {
	if (isBrowser()) {
		window.localStorage.removeItem(ONBOARDING_COMPLETE_KEY);
	}
	onboardingState.set({ ...DEFAULT_ONBOARDING_STATE });
}

export function isOnboardingComplete(): boolean {
	if (!isBrowser()) {
		return false;
	}
	return window.localStorage.getItem(ONBOARDING_COMPLETE_KEY) === 'true';
}
