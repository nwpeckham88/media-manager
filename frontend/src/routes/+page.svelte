<script lang="ts">
	import { onMount } from 'svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SectionHeader from '$lib/components/ui/SectionHeader.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
	import {
		mergeWorkflowProgress,
		nextIncompleteStage,
		WORKFLOW_STAGES,
		workflowLabelFromJobKind,
		workflowProgress,
		type WorkflowProgress as WorkflowProgressState
	} from '$lib/workflow/progress';
	import { appSettings, type DashboardRefreshPolicy } from '$lib/workflow/settings';
	import type {
		JobRecord,
		ApiState,
		DuplicateGroupsSummary,
		IndexItemsSummary,
		FormattingCandidatesSummary,
		GoldenStateProgress,
		RecentJobsResponse
	} from '$lib/types/api';

	type IndexStats = {
		total_indexed: number;
		hashed: number;
		probed: number;
	};

	let { data } = $props<{
		data: {
			indexStats: ApiState<IndexStats>;
			exactDuplicates: ApiState<DuplicateGroupsSummary>;
			semanticDuplicates: ApiState<DuplicateGroupsSummary>;
			metadataQueue: ApiState<IndexItemsSummary>;
			formattingQueue: ApiState<FormattingCandidatesSummary>;
			goldenStateProgress: ApiState<GoldenStateProgress>;
			recentJobs: ApiState<RecentJobsResponse>;
			loadedAt: string;
		};
	}>();

	let indexStatsState = $state<ApiState<IndexStats>>({ ok: false, error: 'Index stats unavailable.' });
	let exactDuplicatesState = $state<ApiState<DuplicateGroupsSummary>>({ ok: false, error: 'Duplicate summary unavailable.' });
	let semanticDuplicatesState = $state<ApiState<DuplicateGroupsSummary>>({ ok: false, error: 'Duplicate summary unavailable.' });
	let metadataQueueState = $state<ApiState<IndexItemsSummary>>({ ok: false, error: 'Metadata queue unavailable.' });
	let formattingQueueState = $state<ApiState<FormattingCandidatesSummary>>({ ok: false, error: 'Formatting queue unavailable.' });
	let goldenStateProgressState = $state<ApiState<GoldenStateProgress>>({
		ok: false,
		error: 'Golden-state progress unavailable.'
	});
	let recentJobsState = $state<ApiState<RecentJobsResponse>>({ ok: false, error: 'Recent jobs unavailable.' });
	let refreshedAtIso = $state('');

	let workflowState = $state<WorkflowProgressState>({
		consolidation: false,
		metadata: false,
		formatting: false,
		verify: false
	});
	let workflowNotice = $state('Workflow progress sync pending.');
	let workflowNextHref = $state('/consolidation');
	let workflowNextLabel = $state('Open Consolidation');
	let refreshTimer: ReturnType<typeof setInterval> | null = null;
	let refreshPolicy = $state<DashboardRefreshPolicy>('running-jobs-only');

	const scorecards = $derived.by(() => {
		const indexed = indexStatsState.ok && indexStatsState.data ? indexStatsState.data.total_indexed : 0;
		const exact = exactDuplicatesState.ok && exactDuplicatesState.data ? exactDuplicatesState.data.total_groups : 0;
		const semantic = semanticDuplicatesState.ok && semanticDuplicatesState.data ? semanticDuplicatesState.data.total_groups : 0;
		const metadataQueue =
			goldenStateProgressState.ok && goldenStateProgressState.data
				? goldenStateProgressState.data.metadata_non_compliant
				: metadataQueueState.ok && metadataQueueState.data
					? metadataQueueState.data.total_items
					: 0;
		const formattingQueue =
			goldenStateProgressState.ok && goldenStateProgressState.data
				? goldenStateProgressState.data.naming_non_compliant
				: formattingQueueState.ok && formattingQueueState.data
					? formattingQueueState.data.total_items
					: 0;
        const providerLabel =
            goldenStateProgressState.ok && goldenStateProgressState.data
                ? goldenStateProgressState.data.metadata_provider.toUpperCase()
                : 'provider';
        const namingLabel =
            goldenStateProgressState.ok && goldenStateProgressState.data
                ? goldenStateProgressState.data.naming_format
                : 'configured format';

		return [
			{ label: 'Indexed Files', value: indexed.toLocaleString(), detail: 'Files currently tracked' },
			{
				label: 'Duplicate Groups',
				value: (exact + semantic).toLocaleString(),
				detail: `Exact ${exact} | Semantic ${semantic}`
			},
			{
				label: 'Metadata Drift',
				value: metadataQueue.toLocaleString(),
				detail: `Needs ${providerLabel} provider alignment`
			},
			{
				label: 'Naming Drift',
				value: formattingQueue.toLocaleString(),
				detail: `Not matching ${namingLabel}`
			}
		];
	});

	function computeDashboardHeuristics(): WorkflowProgressState {
		const indexed = indexStatsState.ok && !!indexStatsState.data && indexStatsState.data.total_indexed > 0;
		const metadataQueueEmpty =
			metadataQueueState.ok && !!metadataQueueState.data && metadataQueueState.data.total_items === 0;
		const formattingQueueEmpty =
			formattingQueueState.ok && !!formattingQueueState.data && formattingQueueState.data.total_items === 0;
		const recentJobs: JobRecord[] = recentJobsState.ok && !!recentJobsState.data ? recentJobsState.data.items : [];
		const hasRunningJobs = recentJobs.some((job: JobRecord) => job.status === 'running');
		const hasAnyTerminalJobs = recentJobs.some(
			(job: JobRecord) => job.status === 'succeeded' || job.status === 'failed' || job.status === 'canceled'
		);

		return {
			consolidation: indexed,
			metadata: indexed && metadataQueueEmpty,
			formatting: indexed && formattingQueueEmpty,
			verify: hasAnyTerminalJobs && !hasRunningJobs
		};
	}

	function updateWorkflowBanner(progress: WorkflowProgressState) {
		const completeCount = WORKFLOW_STAGES.filter((stage) => progress[stage.key]).length;
		const nextStage = nextIncompleteStage(progress);

		if (!nextStage) {
			workflowNotice = 'Workflow complete: all 4 stages are marked complete. Continue monitoring Queue and Operations.';
			workflowNextHref = '/operations';
			workflowNextLabel = 'Open Operations';
			return;
		}

		workflowNotice = `Workflow status: ${completeCount}/4 complete. Next recommended stage: ${nextStage.label}.`;
		workflowNextHref = nextStage.path;
		workflowNextLabel = `Open ${nextStage.label}`;
	}

	function authHeaders(): HeadersInit | undefined {
		const token = window.localStorage.getItem('mm-api-token');
		return token ? { Authorization: `Bearer ${token}` } : undefined;
	}

	async function readJson<T>(path: string): Promise<ApiState<T>> {
		try {
			const response = await window.fetch(path, { headers: authHeaders() });
			if (!response.ok) {
				return {
					ok: false,
					error: `HTTP ${response.status} from ${path}`
				};
			}
			return {
				ok: true,
				data: (await response.json()) as T
			};
		} catch (error) {
			return {
				ok: false,
				error: error instanceof Error ? error.message : `Unknown error calling ${path}`
			};
		}
	}

	function applyInitialData(): void {
		indexStatsState = data.indexStats;
		exactDuplicatesState = data.exactDuplicates;
		semanticDuplicatesState = data.semanticDuplicates;
		metadataQueueState = data.metadataQueue;
		formattingQueueState = data.formattingQueue;
		goldenStateProgressState = data.goldenStateProgress;
		recentJobsState = data.recentJobs;
		refreshedAtIso = data.loadedAt;
	}

	async function refreshDashboardData(): Promise<void> {
		const [indexStats, exactDuplicates, semanticDuplicates, metadataQueue, formattingQueue, goldenStateProgress, recentJobs] =
			await Promise.all([
				readJson<IndexStats>('/api/index/stats'),
				readJson<DuplicateGroupsSummary>('/api/consolidation/exact-duplicates?limit=1&min_group_size=2'),
				readJson<DuplicateGroupsSummary>('/api/consolidation/semantic-duplicates?limit=1&min_group_size=2'),
				readJson<IndexItemsSummary>('/api/index/items?limit=1&offset=0&only_missing_provider=true&max_confidence=0.95'),
				readJson<FormattingCandidatesSummary>('/api/formatting/candidates?limit=1&offset=0'),
				readJson<GoldenStateProgress>('/api/workflow/golden-state-progress'),
				readJson<RecentJobsResponse>('/api/jobs/recent?limit=12')
			]);

		indexStatsState = indexStats;
		exactDuplicatesState = exactDuplicates;
		semanticDuplicatesState = semanticDuplicates;
		metadataQueueState = metadataQueue;
		formattingQueueState = formattingQueue;
		goldenStateProgressState = goldenStateProgress;
		recentJobsState = recentJobs;
		refreshedAtIso = new Date().toISOString();
		mergeWorkflowProgress(computeDashboardHeuristics());
	}

	function hasRunningJobs(): boolean {
		if (!recentJobsState.ok || !recentJobsState.data) {
			return false;
		}
		return recentJobsState.data.items.some((job) => job.status === 'running');
	}

	onMount(() => {
		const settingsUnsub = appSettings.subscribe((value) => {
			refreshPolicy = value.dashboardRefreshPolicy;
		});

		applyInitialData();
		mergeWorkflowProgress(computeDashboardHeuristics());
		void refreshDashboardData();
		refreshTimer = setInterval(() => {
			if (refreshPolicy === 'manual') {
				return;
			}

			if (refreshPolicy === 'always' || hasRunningJobs()) {
				void refreshDashboardData();
			}
		}, 10000);

		const unsubscribe = workflowProgress.subscribe((progress) => {
			workflowState = progress;
			updateWorkflowBanner(progress);
		});

		return () => {
			settingsUnsub();
			unsubscribe();
			if (refreshTimer) {
				clearInterval(refreshTimer);
			}
		};
	});
</script>

<svelte:head>
	<title>Media Manager | Workflow Dashboard</title>
</svelte:head>

<main class="dashboard-shell">
	<PageHero
		eyebrow="Workflow Hub"
		title="Operate Your Jellyfin Library With Guardrails"
		lead="Run staged operations in sequence and keep every action auditable, reversible, and portable beyond this app."
		stamp={`Snapshot: ${new Date(refreshedAtIso).toLocaleString()}`}
	>
		<div class="hero-actions">
			<a href={workflowNextHref}>{workflowNextLabel}</a>
			<a href="/queue">Inspect Queue</a>
			<a href="/operations">Review Operations</a>
		</div>
	</PageHero>

	<OperationResultBanner notice={workflowNotice} nextHref={workflowNextHref} nextLabel={workflowNextLabel} />

	<section class="metrics-grid" aria-label="Library Status Metrics">
		{#each scorecards as card}
			<SurfaceCard className="metric-card" compact>
				<p class="mono metric-label">{card.label}</p>
				<h2 class="metric-value">{card.value}</h2>
				<p class="metric-detail">{card.detail}</p>
			</SurfaceCard>
		{/each}
	</section>

	<section class="row-grid">
		<SurfaceCard className="stage-map">
			<SectionHeader title="Stage Map" href={workflowNextHref} label="Resume Next Stage" />
			<div class="stage-grid">
				{#each WORKFLOW_STAGES as card}
					<article class="stage-card" class:done={workflowState[card.key]}>
						<p class="mono">Stage {card.id}</p>
						<h3>{card.label}</h3>
						<p>{card.description}</p>
						<p class="mono stage-status">{workflowState[card.key] ? 'Complete' : 'Pending'}</p>
						<a href={card.path}>Open {card.label}</a>
					</article>
				{/each}
			</div>
		</SurfaceCard>

		<SurfaceCard className="recent-jobs">
			<SectionHeader title="Recent Jobs" href="/queue" label="Open Full Queue" />
			{#if recentJobsState.ok && recentJobsState.data}
				<ul class="jobs mono">
					{#if recentJobsState.data.items.length === 0}
						<li><span>No jobs yet</span><strong>Idle</strong></li>
					{:else}
						{#each recentJobsState.data.items.slice(0, 8) as job}
							<li>
								<div>
									<span>#{job.id} {job.kind}</span>
									{#if job.error}
										<small>{job.error}</small>
									{/if}
								</div>
								<strong>{workflowLabelFromJobKind(job.kind)} | {job.status}</strong>
							</li>
						{/each}
					{/if}
				</ul>
			{:else}
				<p class="error">{recentJobsState.error ?? 'Unable to read recent jobs.'}</p>
			{/if}
		</SurfaceCard>
	</section>
</main>

<style>
	.dashboard-shell {
		width: min(var(--content-max), 94vw);
		margin: var(--space-4) auto calc(var(--space-6) * 2);
		display: grid;
		gap: var(--space-4);
		animation: rise 260ms ease-out;
	}

	.hero-actions {
		display: flex;
		gap: var(--space-2);
		flex-wrap: wrap;
	}

	.hero-actions a,
	.stage-card a {
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: 0.42rem 0.65rem;
		text-decoration: none;
		font-weight: 700;
		font-size: var(--font-small);
		background: color-mix(in srgb, var(--card) 95%, transparent);
	}

	.metrics-grid {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: var(--space-3);
	}

	.metric-value {
		margin: var(--space-1) 0;
		font-size: 2.05rem;
	}

	.metric-label {
		margin: 0;
		font-size: var(--font-label);
		letter-spacing: 0.09em;
		text-transform: uppercase;
		color: var(--muted);
	}

	.metric-detail {
		margin: 0;
		color: var(--muted);
		font-size: var(--font-small);
	}

	.row-grid {
		display: grid;
		grid-template-columns: 1.65fr 1fr;
		gap: var(--space-3);
	}

	.stage-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--space-2);
	}

	.stage-card {
		display: grid;
		gap: var(--space-2);
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: var(--space-3);
		background: color-mix(in srgb, var(--card) 90%, transparent);
	}

	.stage-card.done {
		border-color: color-mix(in srgb, var(--accent) 48%, var(--ring));
	}

	.stage-card h3,
	.stage-card p {
		margin: 0;
	}

	.stage-card p {
		color: var(--muted);
	}

	.stage-status {
		font-size: var(--font-label);
		letter-spacing: 0.08em;
		text-transform: uppercase;
		font-weight: 700;
	}

	.jobs {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-2);
	}

	.jobs li {
		display: flex;
		justify-content: space-between;
		gap: var(--space-3);
		padding-bottom: var(--space-2);
		border-bottom: 1px dashed var(--ring);
	}

	.jobs span {
		display: block;
		font-size: var(--font-small);
	}

	.jobs small {
		display: block;
		margin-top: var(--space-1);
		font-size: var(--font-label);
		color: var(--danger);
	}

	.error {
		margin: 0;
		color: var(--danger);
		font-weight: 700;
	}

	@keyframes rise {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
		to {
			opacity: 1;
			transform: translateY(0);
		}
	}

	@media (max-width: 1080px) {
		.metrics-grid {
			grid-template-columns: 1fr 1fr;
		}

		.row-grid {
			grid-template-columns: 1fr;
		}
	}

	@media (max-width: 760px) {
		.metrics-grid,
		.stage-grid {
			grid-template-columns: 1fr;
		}

		.jobs li {
			flex-direction: column;
		}
	}
</style>
