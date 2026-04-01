<script lang="ts">
	import { goto } from '$app/navigation';
	import { onDestroy } from 'svelte';
	import { get } from 'svelte/store';
	import {
		completeOnboarding,
		onboardingState,
		updateOnboardingState,
		type HashingMode,
		type MetadataProvider,
		type NamingFormat
	} from '$lib/workflow/onboarding';
	import {
		appSettings,
		updateAppSettings,
		type DashboardRefreshPolicy
	} from '$lib/workflow/settings';
	import IndexingModeSelector from './IndexingModeSelector.svelte';
	import LibraryDetectionPanel from './LibraryDetectionPanel.svelte';
	import MetadataProviderSelector from './MetadataProviderSelector.svelte';
	import NamingFormatSelector from './NamingFormatSelector.svelte';
	import ScanStatusPanel from './ScanStatusPanel.svelte';
	import type {
		ApiState,
		AppConfigResponse,
		IndexStats,
		RecentJobsResponse,
		ScanSummary
	} from './types';

	const persisted = get(onboardingState);
	const settings = get(appSettings);

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
	let hashingMode = $state<HashingMode>(persisted.hashingMode ?? settings.defaultHashingMode);
	let metadataProvider = $state<MetadataProvider>(
		persisted.metadataProvider ?? settings.metadataProvider ?? 'tmdb'
	);
	let namingFormat = $state<NamingFormat>(
		persisted.namingFormat ?? settings.namingFormat ?? 'movie_title_subtitle_year'
	);
	let refreshPolicy = $state<DashboardRefreshPolicy>(settings.dashboardRefreshPolicy);
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
		updateOnboardingState({
			step: normalizeStep(step),
			hashingMode,
			metadataProvider,
			namingFormat
		});
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

	async function startIndexing(): Promise<boolean> {
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
				return false;
			}

			indexingStarted = true;
			kickoffPolling();
			return true;
		} catch (error) {
			startError = error instanceof Error ? error.message : 'Unknown error while starting index';
			return false;
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

	async function saveGoldenStatePreferences(): Promise<boolean> {
		try {
			const token = window.localStorage.getItem('mm-api-token');
			const headers: HeadersInit = {
				'content-type': 'application/json'
			};
			if (token) {
				headers.Authorization = `Bearer ${token}`;
			}

			const response = await window.fetch('/api/config/golden-state', {
				method: 'POST',
				headers,
				body: JSON.stringify({
					metadata_provider: metadataProvider,
					naming_format: namingFormat
				})
			});

			if (!response.ok) {
				startError = `Failed to save golden-state preferences: HTTP ${response.status}`;
				return false;
			}
			return true;
		} catch (error) {
			startError = error instanceof Error ? error.message : 'Unknown error saving preferences';
			return false;
		}
	}

	async function finishSetup() {
		updateAppSettings({
			defaultHashingMode: hashingMode,
			metadataProvider,
			namingFormat,
			dashboardRefreshPolicy: refreshPolicy
		});

		updateOnboardingState({
			step: 4,
			hashingMode,
			metadataProvider,
			namingFormat,
			lastDetectedRoots: scanState.ok && scanState.data ? scanState.data.roots.length : 0,
			lastDetectedMediaFiles: scanState.ok && scanState.data ? scanState.data.total_media_files : 0
		});
		completeOnboarding();
		if (pollTimer) {
			clearInterval(pollTimer);
		}
		await goto('/');
	}

	async function finalizeOnboarding() {
		const saved = await saveGoldenStatePreferences();
		if (!saved) {
			return;
		}

		if (!canFinishWithoutStarting) {
			const started = await startIndexing();
			if (!started) {
				return;
			}
		}

		await finishSetup();
	}
</script>

<section class="wizard-shell">
	<div class="steps" role="tablist" aria-label="Setup Steps">
		<button type="button" class:active={step === 1} onclick={() => (step = 1)}>1. Detect</button>
		<button type="button" class:active={step === 2} onclick={() => canAdvanceFromDetection && (step = 2)}>
			2. Indexing
		</button>
		<button type="button" class:active={step === 3} onclick={() => canAdvanceFromDetection && (step = 3)}>
			3. Golden State
		</button>
		<button type="button" class:active={step === 4} onclick={() => canAdvanceFromDetection && (step = 4)}>
			4. Launch
		</button>
	</div>

	<div class="stage">
		{#if step === 1}
			<LibraryDetectionPanel {configState} {scanState} />
		{:else if step === 2}
			<IndexingModeSelector bind:value={hashingMode} />
		{:else if step === 3}
			<div class="final-step">
				<MetadataProviderSelector bind:value={metadataProvider} />
				<NamingFormatSelector bind:value={namingFormat} />
				<section class="policy-card" aria-label="Workflow Policy">
					<p class="mono label">Workflow Policy</p>
					<p class="muted">
						This defines your golden state. The app will guide metadata and rename stages to converge toward it.
					</p>
					<label>
						<span>Dashboard Refresh Policy</span>
						<select bind:value={refreshPolicy}>
							<option value="running-jobs-only">Only refresh while jobs run (recommended)</option>
							<option value="always">Always refresh every interval</option>
							<option value="manual">Manual refresh only</option>
						</select>
					</label>
				</section>
			</div>
		{:else}
			<div class="final-step">
				<section class="policy-card" aria-label="Ready to Launch">
					<p class="mono label">Ready to Launch</p>
					<p class="muted">
						Indexing starts only when you finish this setup. No background task is created before this step.
					</p>
					<p class="muted">
						If your library was already indexed earlier, setup will complete immediately without starting a new job.
					</p>
				</section>
				<ScanStatusPanel
					indexStatsState={localIndexStats}
					recentJobsState={localRecentJobs}
					{indexingStarted}
					{starting}
					{startError}
				/>
			</div>
		{/if}
	</div>

	<footer class="actions">
		<div class="left">
			{#if step > 1}
				<button type="button" class="ghost" onclick={() => (step -= 1)}>Back</button>
			{/if}
		</div>
		<div class="right">
			{#if step < 4}
				<button type="button" onclick={() => (step += 1)} disabled={step === 1 && !canAdvanceFromDetection}>Continue</button>
			{:else}
				<button type="button" onclick={finalizeOnboarding} disabled={starting}>
					{starting
						? 'Starting...'
						: canFinishWithoutStarting
							? 'Finish Setup'
							: 'Finish Setup and Start Indexing'}
				</button>
			{/if}
		</div>
	</footer>
</section>

<style>
	.wizard-shell {
		display: grid;
		gap: var(--space-4);
	}

	.steps {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: var(--space-2);
	}

	.steps button {
		border: 1px solid var(--ring);
		padding: var(--space-2) var(--space-2);
		border-radius: var(--radius-md);
		font-weight: 700;
		font-size: var(--font-small);
		background: color-mix(in srgb, var(--card) 94%, transparent);
		color: var(--text);
		cursor: pointer;
	}

	.steps button.active {
		border-color: color-mix(in srgb, var(--accent) 55%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 26%, transparent);
	}

	.steps button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	.stage {
		border: 1px solid var(--ring);
		border-radius: 16px;
		padding: var(--space-4);
		background: color-mix(in srgb, var(--card) 94%, transparent);
	}

	.final-step {
		display: grid;
		gap: var(--space-4);
	}

	.policy-card {
		display: grid;
		gap: var(--space-2);
		border: 1px solid var(--ring);
		border-radius: 12px;
		padding: var(--space-3);
		background: color-mix(in srgb, var(--card) 94%, transparent);
	}

	.policy-card .label {
		margin: 0;
		font-size: var(--font-label);
		text-transform: uppercase;
		letter-spacing: 0.09em;
		color: var(--muted);
	}

	.policy-card .muted {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
	}

	.policy-card label {
		display: grid;
		gap: var(--space-2);
		font-weight: 700;
	}

	.policy-card select {
		border: 1px solid var(--ring);
		border-radius: 8px;
		padding: 0.42rem 0.5rem;
		background: color-mix(in srgb, var(--card) 96%, transparent);
		color: var(--text);
		font-size: var(--font-small);
	}

	.actions {
		display: flex;
		justify-content: space-between;
		align-items: center;
	}

	.left,
	.right {
		display: flex;
		gap: var(--space-2);
	}

	.actions button {
		border-radius: var(--radius-md);
		padding: 0.52rem 0.86rem;
		font-weight: 700;
		font-size: var(--font-small);
		border: 1px solid var(--ring);
		cursor: pointer;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
	}

	.actions button.ghost {
		background: transparent;
	}

	.actions button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	@media (max-width: 860px) {
		.steps {
			grid-template-columns: 1fr 1fr;
		}

		.actions {
			flex-direction: column;
			align-items: stretch;
			gap: var(--space-3);
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
