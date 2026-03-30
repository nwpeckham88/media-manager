<script lang="ts">
	import { onMount } from 'svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import {
		mergeWorkflowProgress,
		nextIncompleteStage,
		WORKFLOW_STAGES,
		workflowLabelFromJobKind,
		workflowProgress,
		type WorkflowProgress as WorkflowProgressState
	} from '$lib/workflow/progress';

	type JobRecord = {
		id: number;
		kind: string;
		status: 'running' | 'succeeded' | 'failed' | 'canceled';
		error: string | null;
	};

	type ApiState<T> = {
		ok: boolean;
		data?: T;
		error?: string;
	};

	type IndexStats = {
		total_indexed: number;
		hashed: number;
		probed: number;
	};

	type DuplicateGroupsSummary = {
		total_groups: number;
	};

	type IndexItemsSummary = {
		total_items: number;
	};

	type FormattingCandidatesSummary = {
		total_items: number;
	};

	type RecentJobsResponse = {
		items: JobRecord[];
	};

	let { data } = $props<{
		data: {
			indexStats: ApiState<IndexStats>;
			exactDuplicates: ApiState<DuplicateGroupsSummary>;
			semanticDuplicates: ApiState<DuplicateGroupsSummary>;
			metadataQueue: ApiState<IndexItemsSummary>;
			formattingQueue: ApiState<FormattingCandidatesSummary>;
			recentJobs: ApiState<RecentJobsResponse>;
			loadedAt: string;
		};
	}>();

	let workflowState = $state<WorkflowProgressState>({
		consolidation: false,
		metadata: false,
		formatting: false,
		verify: false
	});
	let workflowNotice = $state('Workflow progress sync pending.');
	let workflowNextHref = $state('/consolidation');
	let workflowNextLabel = $state('Open Consolidation');

	function computeDashboardHeuristics(): WorkflowProgressState {
		const indexed = data.indexStats.ok && !!data.indexStats.data && data.indexStats.data.total_indexed > 0;
		const metadataQueueEmpty = data.metadataQueue.ok && !!data.metadataQueue.data && data.metadataQueue.data.total_items === 0;
		const formattingQueueEmpty = data.formattingQueue.ok && !!data.formattingQueue.data && data.formattingQueue.data.total_items === 0;
		const recentJobs: JobRecord[] = data.recentJobs.ok && !!data.recentJobs.data ? data.recentJobs.data.items : [];
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

		workflowNotice = `Workflow status: ${completeCount}/4 stages complete. Next recommended stage: ${nextStage.label}.`;
		workflowNextHref = nextStage.path;
		workflowNextLabel = `Open ${nextStage.label}`;
	}

	onMount(() => {
		mergeWorkflowProgress(computeDashboardHeuristics());

		const unsubscribe = workflowProgress.subscribe((progress) => {
			workflowState = progress;
			updateWorkflowBanner(progress);
		});

		return unsubscribe;
	});
</script>

<svelte:head>
	<title>Media Manager | Workflow Dashboard</title>
</svelte:head>

<main class="dashboard-shell">
	<section class="hero card">
		<p class="eyebrow">Workflow Hub</p>
		<h1>Media Library Operations</h1>
		<p class="lead">Run your staged flow in order: Consolidation -> Metadata -> Formatting -> Verify.</p>
		<p class="mono stamp">Snapshot loaded at {new Date(data.loadedAt).toLocaleString()}</p>
		<div class="hero-actions">
			<a href="/consolidation">Start/Resume Consolidation</a>
			<a href="/metadata">Continue Metadata</a>
			<a href="/formatting">Continue Formatting</a>
			<a href="/operations">Open Operations</a>
		</div>
	</section>

	<OperationResultBanner notice={workflowNotice} nextHref={workflowNextHref} nextLabel={workflowNextLabel} />

	<section class="metrics-grid">
		<article class="card metric">
			<p class="mono label">Indexed Files</p>
			{#if data.indexStats.ok && data.indexStats.data}
				<h2>{data.indexStats.data.total_indexed}</h2>
				<p class="mono">hashed={data.indexStats.data.hashed} probed={data.indexStats.data.probed}</p>
			{:else}
				<h2>n/a</h2>
				<p class="error">{data.indexStats.error ?? 'index stats unavailable'}</p>
			{/if}
		</article>

		<article class="card metric">
			<p class="mono label">Duplicate Groups</p>
			{#if data.exactDuplicates.ok && data.exactDuplicates.data && data.semanticDuplicates.ok && data.semanticDuplicates.data}
				<h2>{data.exactDuplicates.data.total_groups + data.semanticDuplicates.data.total_groups}</h2>
				<p class="mono">exact={data.exactDuplicates.data.total_groups} semantic={data.semanticDuplicates.data.total_groups}</p>
			{:else}
				<h2>n/a</h2>
				<p class="error">Unable to summarize duplicate groups.</p>
			{/if}
		</article>

		<article class="card metric">
			<p class="mono label">Metadata Review Queue</p>
			{#if data.metadataQueue.ok && data.metadataQueue.data}
				<h2>{data.metadataQueue.data.total_items}</h2>
				<p class="mono">items missing provider IDs or low-confidence metadata</p>
			{:else}
				<h2>n/a</h2>
				<p class="error">{data.metadataQueue.error ?? 'metadata queue unavailable'}</p>
			{/if}
		</article>

		<article class="card metric">
			<p class="mono label">Formatting Candidates</p>
			{#if data.formattingQueue.ok && data.formattingQueue.data}
				<h2>{data.formattingQueue.data.total_items}</h2>
				<p class="mono">rename candidates from current indexed snapshot</p>
			{:else}
				<h2>n/a</h2>
				<p class="error">{data.formattingQueue.error ?? 'formatting queue unavailable'}</p>
			{/if}
		</article>
	</section>

	<section class="card">
		<h2>Stage Map</h2>
		<div class="stage-grid">
			{#each WORKFLOW_STAGES as card}
				<article class="stage-card">
					<p class="mono">Stage {card.id}</p>
					<h3>{card.label}</h3>
					<p>{card.description}</p>
					<p class="mono stage-status" class:done={workflowState[card.key]}>
						{workflowState[card.key] ? 'Complete' : 'Pending'}
					</p>
					<a href={card.path}>Open {card.label}</a>
				</article>
			{/each}
		</div>
	</section>

	<section class="card">
		<div class="split-head">
			<h2>Recent Jobs</h2>
			<a href="/queue">Open Full Queue</a>
		</div>
		{#if data.recentJobs.ok && data.recentJobs.data}
			<ul class="jobs mono">
				{#if data.recentJobs.data.items.length === 0}
					<li><span>No jobs yet</span><strong>Idle</strong></li>
				{:else}
					{#each data.recentJobs.data.items.slice(0, 8) as job}
						<li>
							<span>#{job.id} {job.kind}</span>
							<strong>{workflowLabelFromJobKind(job.kind)} | {job.status}</strong>
						</li>
					{/each}
				{/if}
			</ul>
		{:else}
			<p class="error">{data.recentJobs.error ?? 'Unable to read recent jobs.'}</p>
		{/if}
	</section>
</main>

<style>
	.dashboard-shell {
		width: min(1160px, 94vw);
		margin: 1rem auto 3rem;
		display: grid;
		gap: 0.9rem;
	}

	.card {
		background: color-mix(in srgb, var(--card) 92%, transparent);
		border: 1px solid var(--ring);
		border-radius: 14px;
		padding: 0.95rem;
		backdrop-filter: blur(2px);
	}

	.eyebrow {
		margin: 0;
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-size: 0.76rem;
		color: var(--muted);
		font-weight: 700;
	}

	h1 {
		margin: 0.3rem 0;
	}

	.lead {
		margin: 0;
		color: var(--muted);
	}

	.stamp {
		margin: 0.4rem 0 0;
		font-size: 0.76rem;
		color: var(--muted);
	}

	.hero-actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.55rem;
		margin-top: 0.8rem;
	}

	.hero-actions a,
	.stage-card a,
	.split-head a {
		border: 1px solid var(--ring);
		border-radius: 10px;
		padding: 0.42rem 0.6rem;
		text-decoration: none;
		font-weight: 700;
		background: color-mix(in srgb, var(--card) 94%, transparent);
	}

	.metrics-grid {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 0.7rem;
	}

	.metric h2 {
		margin: 0.25rem 0;
		font-size: 2rem;
	}

	.label {
		margin: 0;
		font-size: 0.74rem;
		color: var(--muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}

	.stage-grid {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 0.7rem;
	}

	.stage-card {
		border: 1px solid var(--ring);
		border-radius: 10px;
		padding: 0.72rem;
		background: color-mix(in srgb, var(--card) 90%, transparent);
		display: grid;
		gap: 0.5rem;
	}

	.stage-card h3,
	.stage-card p,
	.split-head h2 {
		margin: 0;
	}

	.stage-card p {
		color: var(--muted);
	}

	.stage-status {
		font-size: 0.75rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		font-weight: 700;
	}

	.stage-status.done {
		color: var(--accent);
	}

	.split-head {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 0.5rem;
	}

	.jobs {
		list-style: none;
		padding: 0;
		margin: 0;
		display: grid;
		gap: 0.45rem;
	}

	.jobs li {
		display: flex;
		justify-content: space-between;
		gap: 0.65rem;
		padding-bottom: 0.35rem;
		border-bottom: 1px dashed var(--ring);
	}

	.error {
		margin: 0;
		color: var(--danger);
		font-weight: 700;
	}

	@media (max-width: 1050px) {
		.metrics-grid,
		.stage-grid {
			grid-template-columns: 1fr 1fr;
		}
	}

	@media (max-width: 700px) {
		.metrics-grid,
		.stage-grid {
			grid-template-columns: 1fr;
		}

		.split-head {
			flex-direction: column;
			align-items: flex-start;
			gap: 0.5rem;
		}
	}
</style>
