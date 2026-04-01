<script lang="ts">
	import type { ApiState, IndexStats, RecentJobsResponse } from './types';

	let {
		indexStatsState,
		recentJobsState,
		indexingStarted,
		starting,
		startError
	} = $props<{
		indexStatsState: ApiState<IndexStats>;
		recentJobsState: ApiState<RecentJobsResponse>;
		indexingStarted: boolean;
		starting: boolean;
		startError: string;
	}>();

	const hasRunningJobs = $derived.by(() => {
		if (!recentJobsState.ok || !recentJobsState.data) {
			return false;
		}
		return recentJobsState.data.items.some((job: { status: string }) => job.status === 'running');
	});

	const latestJob = $derived.by(() => {
		if (!recentJobsState.ok || !recentJobsState.data || recentJobsState.data.items.length === 0) {
			return null;
		}
		return recentJobsState.data.items[0];
	});
</script>

<section class="panel">
	<header>
		<p class="eyebrow">Step 4</p>
		<h2>Scan + Index Status</h2>
		<p>Watch detection and indexing progress before entering the main workflow hub.</p>
	</header>

	{#if startError}
		<p class="error">{startError}</p>
	{/if}

	<div class="metrics">
		<article>
			<p class="label mono">Indexed</p>
			<strong>{indexStatsState.ok && indexStatsState.data ? indexStatsState.data.total_indexed : 0}</strong>
		</article>
		<article>
			<p class="label mono">Hashed</p>
			<strong>{indexStatsState.ok && indexStatsState.data ? indexStatsState.data.hashed : 0}</strong>
		</article>
		<article>
			<p class="label mono">Probed</p>
			<strong>{indexStatsState.ok && indexStatsState.data ? indexStatsState.data.probed : 0}</strong>
		</article>
	</div>

	<div class="status-row">
		<span class:active={starting || hasRunningJobs} class="pill">
			{#if starting}
				Starting index job...
			{:else if hasRunningJobs}
				Indexing in progress
			{:else if indexingStarted}
				Index request submitted
			{:else}
				Index not started from setup yet
			{/if}
		</span>

		{#if latestJob}
			<span class="mono latest">Latest job: #{latestJob.id} {latestJob.kind} ({latestJob.status})</span>
		{/if}
	</div>
</section>

<style>
	.panel {
		display: grid;
		gap: var(--space-4);
	}

	header h2 {
		margin: var(--space-1) 0;
		font-size: 1.3rem;
	}

	header p {
		margin: 0;
		color: var(--muted);
	}

	.eyebrow {
		margin: 0;
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-size: var(--font-label);
		font-weight: 700;
		color: var(--muted);
	}

	.metrics {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: var(--space-3);
	}

	article {
		border: 1px solid var(--ring);
		border-radius: 12px;
		padding: var(--space-3);
		background: color-mix(in srgb, var(--card) 92%, transparent);
	}

	.label {
		margin: 0;
		font-size: var(--font-label);
		letter-spacing: 0.09em;
		text-transform: uppercase;
		color: var(--muted);
	}

	strong {
		font-size: 1.35rem;
	}

	.status-row {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		flex-wrap: wrap;
	}

	.pill {
		border-radius: 999px;
		padding: 0.4rem 0.62rem;
		font-weight: 700;
		font-size: var(--font-small);
		border: 1px solid var(--ring);
		background: color-mix(in srgb, var(--card) 94%, transparent);
	}

	.pill.active {
		border-color: color-mix(in srgb, var(--accent) 56%, var(--ring));
		background: color-mix(in srgb, var(--accent) 12%, transparent);
	}

	.latest {
		font-size: var(--font-small);
		color: var(--muted);
	}

	.error {
		margin: 0;
		color: var(--danger);
		font-weight: 700;
	}

	@media (max-width: 820px) {
		.metrics {
			grid-template-columns: 1fr;
		}
	}
</style>
