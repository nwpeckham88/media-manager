<script lang="ts">
	import { goto } from '$app/navigation';
	import { onDestroy } from 'svelte';
	import { get } from 'svelte/store';
	import {
		completeOnboarding,
		onboardingState,
		updateOnboardingState,
		type HashingMode,
		type RenamePreset
	} from '$lib/workflow/onboarding';
	import IndexingModeSelector from './IndexingModeSelector.svelte';
	import LibraryDetectionPanel from './LibraryDetectionPanel.svelte';
	import RenamePresetSelector from './RenamePresetSelector.svelte';
	import ScanStatusPanel from './ScanStatusPanel.svelte';
	import type {
		ApiState,
		AppConfigResponse,
		IndexStats,
		RecentJobsResponse,
		ScanSummary
	} from './types';

	const persisted = get(onboardingState);

	let {
		configState,
		scanState,
		indexStatsState,
		recentJobsState
	} = $props<{
		configState: ApiState<AppConfigResponse>;
		scanState: ApiState<ScanSummary>;
		indexStatsState: ApiState<IndexStats>;
		recentJobsState: ApiState<RecentJobsResponse>;
	}>();

	let step = $state<number>(persisted.step);
	let hashingMode = $state<HashingMode>(persisted.hashingMode);
	let renamePreset = $state<RenamePreset>(persisted.renamePreset);
	let starting = $state(false);
	let indexingStarted = $state(false);
	let startError = $state('');
	let localIndexStats = $state<ApiState<IndexStats>>({ ok: false, error: 'Index stats unavailable.' });
	let localRecentJobs = $state<ApiState<RecentJobsResponse>>({ ok: false, error: 'Recent jobs unavailable.' });

	const canAdvanceFromDetection = $derived.by(() => {
		if (!scanState.ok || !scanState.data) {
			return false;
		}
		return scanState.data.roots.length > 0;
	});

	const canFinishWithoutStarting = $derived.by(() => {
		if (!localIndexStats.ok || !localIndexStats.data) {
			return false;
		}
		return localIndexStats.data.total_indexed > 0;
	});

	$effect(() => {
		updateOnboardingState({ step: normalizeStep(step), hashingMode, renamePreset });
	});

	$effect(() => {
		localIndexStats = indexStatsState;
		localRecentJobs = recentJobsState;
	});

	let pollTimer: ReturnType<typeof setInterval> | null = null;

	onDestroy(() => {
		if (pollTimer) {
			clearInterval(pollTimer);
		}
	});

	async function startIndexing() {
		startError = '';
		starting = true;

		try {
			const token = window.localStorage.getItem('mm-api-token');
			const headers: HeadersInit = {
				'content-type': 'application/json'
			};
			if (token) {
				headers.Authorization = `Bearer ${token}`;
			}

			const response = await window.fetch('/api/index/start', {
				method: 'POST',
				headers,
				body: JSON.stringify({
					include_probe: true,
					include_hashes: hashingMode === 'strict'
				})
			});

			if (!response.ok) {
				startError = `Failed to start index: HTTP ${response.status}`;
				return;
			}

			indexingStarted = true;
			step = 4;
			kickoffPolling();
		} catch (error) {
			startError = error instanceof Error ? error.message : 'Unknown error while starting index';
		} finally {
			starting = false;
		}
	}

	function kickoffPolling(): void {
		if (pollTimer) {
			clearInterval(pollTimer);
		}

		void refreshStatus();
		pollTimer = setInterval(() => {
			void refreshStatus();
		}, 4000);
	}

	async function refreshStatus(): Promise<void> {
		const token = window.localStorage.getItem('mm-api-token');
		const headers = token ? { Authorization: `Bearer ${token}` } : undefined;

		try {
			const [statsResponse, jobsResponse] = await Promise.all([
				window.fetch('/api/index/stats', { headers }),
				window.fetch('/api/jobs/recent?limit=8', { headers })
			]);

			if (statsResponse.ok) {
				localIndexStats = { ok: true, data: (await statsResponse.json()) as IndexStats };
			}
			if (jobsResponse.ok) {
				localRecentJobs = { ok: true, data: (await jobsResponse.json()) as RecentJobsResponse };
			}
		} catch {
			// Polling is best-effort; non-fatal while onboarding proceeds.
		}
	}

	function normalizeStep(value: number): 1 | 2 | 3 | 4 {
		if (value <= 1) {
			return 1;
		}
		if (value === 2) {
			return 2;
		}
		if (value === 3) {
			return 3;
		}
		return 4;
	}

	async function finishSetup() {
		updateOnboardingState({
			step: 4,
			hashingMode,
			renamePreset,
			lastDetectedRoots: scanState.ok && scanState.data ? scanState.data.roots.length : 0,
			lastDetectedMediaFiles: scanState.ok && scanState.data ? scanState.data.total_media_files : 0
		});
		completeOnboarding();
		if (pollTimer) {
			clearInterval(pollTimer);
		}
		await goto('/');
	}
</script>

<section class="wizard-shell">
	<div class="steps" role="tablist" aria-label="Setup Steps">
		<button type="button" class:active={step === 1} onclick={() => (step = 1)}>1. Detect</button>
		<button type="button" class:active={step === 2} onclick={() => canAdvanceFromDetection && (step = 2)}>
			2. Status
		</button>
		<button type="button" class:active={step === 3} onclick={() => canAdvanceFromDetection && (step = 3)}>
			3. Indexing
		</button>
		<button type="button" class:active={step === 4} onclick={() => (step = 4)}>4. Naming</button>
	</div>

	<div class="stage">
		{#if step === 1}
			<LibraryDetectionPanel {configState} {scanState} />
		{:else if step === 2}
			<ScanStatusPanel
				indexStatsState={localIndexStats}
				recentJobsState={localRecentJobs}
				{indexingStarted}
				{starting}
				{startError}
			/>
		{:else if step === 3}
			<IndexingModeSelector bind:value={hashingMode} />
		{:else}
			<RenamePresetSelector bind:value={renamePreset} />
		{/if}
	</div>

	<footer class="actions">
		<div class="left">
			{#if step > 1}
				<button type="button" class="ghost" onclick={() => (step -= 1)}>Back</button>
			{/if}
		</div>
		<div class="right">
			{#if step < 3}
				<button type="button" onclick={() => (step += 1)} disabled={step === 1 && !canAdvanceFromDetection}>Continue</button>
			{:else if step === 3}
				<button type="button" onclick={startIndexing} disabled={starting}>
					{starting ? 'Starting...' : 'Start Indexing'}
				</button>
			{:else}
				<button type="button" onclick={finishSetup} disabled={!indexingStarted && !canFinishWithoutStarting}>
					Enter Dashboard
				</button>
			{/if}
		</div>
	</footer>
</section>

<style>
	.wizard-shell {
		display: grid;
		gap: 0.9rem;
	}

	.steps {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 0.5rem;
	}

	.steps button {
		border: 1px solid var(--ring);
		padding: 0.55rem 0.5rem;
		border-radius: 10px;
		font-weight: 700;
		background: color-mix(in srgb, var(--card) 94%, transparent);
		color: var(--text);
		cursor: pointer;
	}

	.steps button.active {
		border-color: color-mix(in srgb, var(--accent) 55%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 26%, transparent);
	}

	.steps button:disabled {
		opacity: 0.55;
		cursor: not-allowed;
	}

	.stage {
		border: 1px solid var(--ring);
		border-radius: 16px;
		padding: 1rem;
		background: color-mix(in srgb, var(--card) 94%, transparent);
	}

	.actions {
		display: flex;
		justify-content: space-between;
		align-items: center;
	}

	.left,
	.right {
		display: flex;
		gap: 0.5rem;
	}

	.actions button {
		border-radius: 10px;
		padding: 0.52rem 0.86rem;
		font-weight: 700;
		border: 1px solid var(--ring);
		cursor: pointer;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
	}

	.actions button.ghost {
		background: transparent;
	}

	.actions button:disabled {
		opacity: 0.55;
		cursor: not-allowed;
	}

	@media (max-width: 860px) {
		.steps {
			grid-template-columns: 1fr 1fr;
		}

		.actions {
			flex-direction: column;
			align-items: stretch;
			gap: 0.6rem;
		}

		.left,
		.right {
			justify-content: stretch;
		}

		.actions button {
			width: 100%;
		}
	}
</style>
