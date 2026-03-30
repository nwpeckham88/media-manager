<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import {
		markStageComplete,
		markStageIncomplete,
		workflowLabelFromJobKind,
		workflowStageFromJobKind,
		type WorkflowStage
	} from '$lib/workflow/progress';

	type OperationEvent = {
		timestamp_ms: number;
		kind: string;
		detail: string;
		success: boolean;
	};

	type JobRecord = {
		id: number;
		kind: string;
		status: 'running' | 'succeeded' | 'failed' | 'canceled';
		updated_at_ms: number;
		result_json: string | null;
		error: string | null;
	};

	type RecentJobsResponse = {
		items: JobRecord[];
	};

	type BulkRollbackResponse = {
		total_items: number;
		succeeded: number;
		failed: number;
		items: Array<{
			operation_id: string;
			success: boolean;
			detail: string | null;
			error: string | null;
		}>;
	};

	type ConfirmDialogState = {
		open: boolean;
		title: string;
		message: string;
		confirmLabel: string;
		tone: 'default' | 'danger';
		busy: boolean;
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

	async function apiFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
		const headers = new Headers(init?.headers ?? {});
		const token = localStorage.getItem('mm-api-token');
		if (token && token.trim().length > 0) {
			headers.set('Authorization', `Bearer ${token.trim()}`);
		}

		return fetch(input, {
			...init,
			headers
		});
	}
</script>

<svelte:head>
	<title>Media Manager | Operations</title>
</svelte:head>

<main class="ops-shell">
	<section class="hero">
		<p class="eyebrow">Operations</p>
		<h1>Apply and Rollback History</h1>
		<p class="lead">Centralized timeline for stage operations. Use this view when you need to audit or recover changes.</p>
	</section>

	<section class="card">
		<div class="actions">
			<button type="button" onclick={refresh} disabled={loading}>Refresh</button>
			<a href="/queue">Open Queue</a>
		</div>
		<OperationResultBanner notice={notice} error={error} nextHref="/queue" nextLabel="Next: Verify in Queue" />
		{#if lastRollback}
			<p class="mono">rollback total={lastRollback.total_items} ok={lastRollback.succeeded} fail={lastRollback.failed}</p>
		{/if}
	</section>

	<section class="card">
		<h2>Recent Jobs With Rollback Candidates</h2>
		{#if jobs.length === 0}
			<p class="mono">No jobs available.</p>
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
	</section>

	<section class="card">
		<h2>Recent Operation Events</h2>
		{#if events.length === 0}
			<p class="mono">No operation events yet.</p>
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
		width: min(1100px, 92vw);
		margin: 1rem auto 3rem;
		display: grid;
		gap: 0.9rem;
	}

	.hero {
		padding: 0.4rem 0;
	}

	.eyebrow {
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-size: 0.78rem;
		color: var(--muted);
		font-weight: 700;
		margin: 0;
	}

	.lead {
		margin: 0;
		max-width: 72ch;
		color: var(--muted);
	}

	.card {
		background: color-mix(in srgb, var(--card) 92%, transparent);
		border: 1px solid var(--ring);
		border-radius: 14px;
		padding: 0.95rem;
		backdrop-filter: blur(2px);
	}

	.actions {
		display: flex;
		gap: 0.6rem;
		margin-bottom: 0.65rem;
		flex-wrap: wrap;
	}

	button,
	a {
		border: 1px solid var(--ring);
		border-radius: 10px;
		padding: 0.42rem 0.62rem;
		font: inherit;
		font-weight: 700;
		text-decoration: none;
		cursor: pointer;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
	}

	.rows {
		list-style: none;
		padding: 0;
		margin: 0;
		display: grid;
		gap: 0.45rem;
	}

	.rows li {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 0.7rem;
		padding-bottom: 0.4rem;
		border-bottom: 1px dashed var(--ring);
	}

	.rows p {
		margin: 0;
	}

	.hint {
		font-size: 0.76rem;
		color: var(--muted);
	}

	.row-actions {
		display: flex;
		gap: 0.45rem;
	}

	.ok {
		color: var(--accent);
	}

	.fail {
		color: var(--danger);
	}
</style>
