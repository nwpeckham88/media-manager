<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
	import SectionHeader from '$lib/components/ui/SectionHeader.svelte';
	import {
		markStageComplete,
		markStageIncomplete,
		workflowLabelFromJobKind,
		workflowStageFromJobKind,
		type WorkflowStage
	} from '$lib/workflow/progress';
	import { apiFetch } from '$lib/utils/api';
	import type { JobRecord, RecentJobsResponse, BulkRollbackResponse, ConfirmDialogState } from '$lib/types/api';

	type OperationEvent = {
		timestamp_ms: number;
		kind: string;
		detail: string;
		success: boolean;
	};

	let loading = $state(false);
	let error = $state('');
	let notice = $state('');
	let events = $state<OperationEvent[]>([]);
	let jobs = $state<JobRecord[]>([]);
	let lastRollback = $state<BulkRollbackResponse | null>(null);
	let confirmDialog = $state<ConfirmDialogState>({
		open: false,
		title: '',
		message: '',
		confirmLabel: 'Confirm',
		tone: 'danger',
		busy: false
	});
	let pendingConfirmAction = $state<null | (() => Promise<void>)>(null);

	onMount(async () => {
		await refresh();
	});

	async function refresh() {
		loading = true;
		error = '';
		await Promise.all([loadOperations(), loadJobs()]);
		loading = false;
	}

	async function loadOperations() {
		const response = await apiFetch('/api/operations/recent?limit=120');
		if (!response.ok) {
			error = `Unable to load operations (${response.status})`;
			return;
		}
		events = (await response.json()) as OperationEvent[];
	}

	async function loadJobs() {
		const response = await apiFetch('/api/jobs/recent?limit=80');
		if (!response.ok) {
			error = `Unable to load jobs (${response.status})`;
			return;
		}
		const payload = (await response.json()) as RecentJobsResponse;
		jobs = payload.items;

		const hasRunningJobs = payload.items.some((job) => job.status === 'running');
		if (!hasRunningJobs && payload.items.length > 0) {
			markStageComplete('verify');
		}
	}

	function extractOperationIdsFromResult(resultJson: string | null): string[] {
		if (!resultJson) {
			return [];
		}

		try {
			const parsed = JSON.parse(resultJson) as {
				items?: Array<{ operation_id?: string | null }>;
			};
			return (parsed.items ?? [])
				.map((item) => item.operation_id)
				.filter((value): value is string => typeof value === 'string' && value.trim().length > 0);
		} catch {
			return [];
		}
	}

	function openConfirmDialog(
		title: string,
		message: string,
		confirmLabel: string,
		action: () => Promise<void>
	) {
		confirmDialog = {
			open: true,
			title,
			message,
			confirmLabel,
			tone: 'danger',
			busy: false
		};
		pendingConfirmAction = action;
	}

	function closeConfirmDialog() {
		if (confirmDialog.busy) {
			return;
		}
		confirmDialog = { ...confirmDialog, open: false };
		pendingConfirmAction = null;
	}

	async function runConfirmDialogAction() {
		if (!pendingConfirmAction) {
			return;
		}
		confirmDialog = { ...confirmDialog, busy: true };
		try {
			await pendingConfirmAction();
		} finally {
			confirmDialog = { ...confirmDialog, open: false, busy: false };
			pendingConfirmAction = null;
		}
	}

	function rollbackFromJob(job: JobRecord) {
		const operationIds = extractOperationIdsFromResult(job.result_json);
		if (operationIds.length === 0) {
			error = `No rollback operation IDs were found in job #${job.id}.`;
			return;
		}

		openConfirmDialog(
			`Rollback operations from job #${job.id}?`,
			`This will attempt to restore ${operationIds.length} operation(s).`,
			'Rollback',
			() => runRollback(operationIds, workflowStageFromJobKind(job.kind))
		);
	}

	async function runRollback(operationIds: string[], sourceStage: WorkflowStage | null) {
		error = '';
		notice = '';
		lastRollback = null;
		const response = await apiFetch('/api/bulk/rollback', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ operation_ids: operationIds })
		});
		if (!response.ok) {
			error = await response.text();
			return;
		}
		lastRollback = (await response.json()) as BulkRollbackResponse;
		notice = `Rollback complete: ok=${lastRollback.succeeded}, fail=${lastRollback.failed}.`;
		if (lastRollback.failed === 0 && sourceStage && sourceStage !== 'verify') {
			markStageIncomplete(sourceStage);
		}
		markStageComplete('verify');
		await refresh();
	}

</script>

<svelte:head>
	<title>Media Manager | Operations</title>
</svelte:head>

<main class="ops-shell">
	<PageHero
		eyebrow="Operations"
		title="Apply and Rollback History"
		lead="Centralized timeline for stage operations. Use this view when you need to audit or recover changes."
	/>

	<section class="stage-card">
		<SurfaceCard as="div">
			<div class="actions">
			<button type="button" onclick={refresh} disabled={loading}>Refresh</button>
			<a href="/queue">Open Queue</a>
			</div>
			<OperationResultBanner notice={notice} error={error} nextHref="/queue" nextLabel="Next: Verify in Queue" />
		{#if lastRollback}
			<p class="mono summary-line">rollback total={lastRollback.total_items} ok={lastRollback.succeeded} fail={lastRollback.failed}</p>
		{/if}
		</SurfaceCard>
	</section>

	<section class="stage-card">
		<SurfaceCard as="div">
			<SectionHeader title="Recent Jobs With Rollback Candidates" />
		{#if jobs.length === 0}
			<p class="mono summary-line">No jobs available.</p>
		{:else}
			<ul class="rows mono">
				{#each jobs as job}
					{@const operationIds = extractOperationIdsFromResult(job.result_json)}
					<li>
						<div>
							<p>#{job.id} {job.kind}</p>
							<p class="hint">stage={workflowLabelFromJobKind(job.kind)} status={job.status} ids={operationIds.length}</p>
						</div>
						<div class="row-actions">
							<button type="button" onclick={() => rollbackFromJob(job)} disabled={operationIds.length === 0 || loading}>Rollback</button>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
		</SurfaceCard>
	</section>

	<section class="stage-card">
		<SurfaceCard as="div">
			<SectionHeader title="Recent Operation Events" />
		{#if events.length === 0}
			<p class="mono summary-line">No operation events yet.</p>
		{:else}
			<ul class="rows mono">
				{#each events as event}
					<li>
						<div>
							<p>{new Date(event.timestamp_ms).toLocaleString()}</p>
							<p class="hint">{event.kind}</p>
						</div>
						<strong class:ok={event.success} class:fail={!event.success}>{event.success ? 'ok' : 'fail'}</strong>
					</li>
				{/each}
			</ul>
		{/if}
		</SurfaceCard>
	</section>

	<ConfirmDialog
		open={confirmDialog.open}
		title={confirmDialog.title}
		message={confirmDialog.message}
		confirmLabel={confirmDialog.confirmLabel}
		tone={confirmDialog.tone}
		busy={confirmDialog.busy}
		onConfirm={runConfirmDialogAction}
		onCancel={closeConfirmDialog}
	/>
</main>

<style>
	.ops-shell {
		width: min(var(--content-max), 94vw);
		margin: var(--space-4) auto calc(var(--space-6) * 2);
		display: grid;
		gap: var(--space-4);
	}

	.stage-card {
		display: grid;
		gap: var(--space-3);
	}

	.summary-line {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
	}

	.actions {
		display: flex;
		gap: var(--space-2);
		margin-bottom: var(--space-3);
		flex-wrap: wrap;
	}

	button,
	a {
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: 0.42rem 0.62rem;
		font: inherit;
		font-weight: 700;
		font-size: var(--font-small);
		text-decoration: none;
		cursor: pointer;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
	}

	button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	.rows {
		list-style: none;
		padding: 0;
		margin: 0;
		display: grid;
		gap: var(--space-2);
	}

	.rows li {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: var(--space-3);
		padding-bottom: var(--space-2);
		border-bottom: 1px dashed var(--ring);
	}

	.rows p {
		margin: 0;
	}

	.hint {
		font-size: var(--font-label);
		color: var(--muted);
	}

	.row-actions {
		display: flex;
		gap: var(--space-2);
	}

	.ok {
		color: var(--accent);
	}

	.fail {
		color: var(--danger);
	}

	@media (max-width: 760px) {
		button,
		a {
			width: 100%;
			text-align: center;
		}

		.rows li {
			flex-direction: column;
			align-items: flex-start;
		}

		.row-actions {
			width: 100%;
		}
	}
</style>
