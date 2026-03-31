<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SectionHeader from '$lib/components/ui/SectionHeader.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
	import {
		markStageComplete,
		markStageIncomplete,
		workflowLabelFromJobKind,
		workflowStageFromJobKind
	} from '$lib/workflow/progress';

	type JobRecord = {
		id: number;
		kind: string;
		status: 'running' | 'succeeded' | 'failed' | 'canceled';
		created_at_ms: number;
		updated_at_ms: number;
		payload_json: string;
		result_json: string | null;
		error: string | null;
	};

	type BulkJobSummary = {
		total: number;
		succeeded: number;
		failed: number;
	};

	type QueueStatusFilter = 'all' | 'running' | 'succeeded' | 'failed' | 'canceled';

	type RecentJobsResponse = {
		total_count: number;
		offset: number;
		limit: number;
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

	type BulkRollbackItem = {
		operation_id: string;
		success: boolean;
		detail: string | null;
		error: string | null;
	};

	type ConfirmDialogState = {
		open: boolean;
		title: string;
		message: string;
		confirmLabel: string;
		tone: 'default' | 'danger';
		busy: boolean;
	};

	let jobs = $state<JobRecord[]>([]);
	let loading = $state(false);
	let error = $state('');
	let activeJobId = $state<number | null>(null);
	let expandedJobIds = $state<number[]>([]);
	let statusFilter = $state<QueueStatusFilter>('all');
	let kindFilter = $state('');
	let notice = $state('');
	let offset = $state(0);
	let pageSize = $state(40);
	let totalCount = $state(0);
	let lastRollbackItems = $state<BulkRollbackItem[]>([]);
	let confirmDialog = $state<ConfirmDialogState>({
		open: false,
		title: '',
		message: '',
		confirmLabel: 'Confirm',
		tone: 'default',
		busy: false
	});
	let pendingConfirmAction = $state<null | (() => Promise<void>)>(null);

	onMount(async () => {
		await loadJobs();
	});

	async function loadJobs() {
		loading = true;
		error = '';
		notice = '';
		const params = new URLSearchParams();
		params.set('limit', String(pageSize));
		params.set('offset', String(offset));
		if (statusFilter !== 'all') {
			params.set('status', statusFilter);
		}
		if (kindFilter.trim().length > 0) {
			params.set('kind', kindFilter.trim());
		}

		const response = await apiFetch(`/api/jobs/recent?${params.toString()}`);
		if (!response.ok) {
			error = `Unable to load jobs (${response.status})`;
			loading = false;
			return;
		}

		const payload = (await response.json()) as RecentJobsResponse;
		totalCount = payload.total_count;
		offset = payload.offset;
		pageSize = payload.limit;
		jobs = payload.items;

		const hasRunningJobs = payload.items.some((job) => job.status === 'running');
		if (!hasRunningJobs && payload.items.length > 0) {
			markStageComplete('verify');
		}
		loading = false;
	}

	function parseBulkSummary(resultJson: string | null): BulkJobSummary | null {
		if (!resultJson) {
			return null;
		}

		try {
			const parsed = JSON.parse(resultJson) as {
				total_items?: number;
				succeeded?: number;
				failed?: number;
			};
			if (typeof parsed.total_items !== 'number') {
				return null;
			}

			return {
				total: parsed.total_items,
				succeeded: typeof parsed.succeeded === 'number' ? parsed.succeeded : 0,
				failed: typeof parsed.failed === 'number' ? parsed.failed : 0
			};
		} catch {
			return null;
		}
	}

	function toggleDetails(jobId: number) {
		if (expandedJobIds.includes(jobId)) {
			expandedJobIds = expandedJobIds.filter((id) => id !== jobId);
			return;
		}

		expandedJobIds = [...expandedJobIds, jobId];
	}

	function formatJson(value: string | null): string {
		if (!value) {
			return 'null';
		}

		try {
			const parsed = JSON.parse(value);
			return JSON.stringify(parsed, null, 2);
		} catch {
			return value;
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

	function extractRollbackItems(resultJson: string | null): BulkRollbackItem[] {
		if (!resultJson) {
			return [];
		}

		try {
			const parsed = JSON.parse(resultJson) as { items?: BulkRollbackItem[] };
			return parsed.items ?? [];
		} catch {
			return [];
		}
	}

	async function applyFilters() {
		offset = 0;
		expandedJobIds = [];
		await loadJobs();
	}

	async function nextPage() {
		if (offset + pageSize >= totalCount) {
			return;
		}

		offset += pageSize;
		expandedJobIds = [];
		await loadJobs();
	}

	async function previousPage() {
		if (offset === 0) {
			return;
		}

		offset = Math.max(0, offset - pageSize);
		expandedJobIds = [];
		await loadJobs();
	}

	async function copyJson(value: string | null, label: string) {
		const text = formatJson(value);
		try {
			await navigator.clipboard.writeText(text);
			notice = `Copied: ${label}.`;
		} catch {
			error = `Unable to copy ${label.toLowerCase()} to clipboard.`;
		}
	}

	function openConfirmDialog(
		title: string,
		message: string,
		confirmLabel: string,
		tone: 'default' | 'danger',
		action: () => Promise<void>
	) {
		confirmDialog = {
			open: true,
			title,
			message,
			confirmLabel,
			tone,
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

	function cancelJob(jobId: number) {
		openConfirmDialog(
			`Cancel job #${jobId}?`,
			'This will request cancellation for the running job.',
			'Cancel Job',
			'danger',
			() => performCancelJob(jobId)
		);
	}

	async function performCancelJob(jobId: number) {
		activeJobId = jobId;
		error = '';
		const response = await apiFetch('/api/jobs/cancel', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ job_id: jobId })
		});

		if (!response.ok) {
			error = await response.text();
			activeJobId = null;
			return;
		}

		await loadJobs();
		notice = `Cancel requested for job #${jobId}.`;
		markStageIncomplete('verify');
		activeJobId = null;
	}

	function retryJob(jobId: number) {
		openConfirmDialog(
			`Retry job #${jobId}?`,
			'Retrying creates a new job record with the original payload.',
			'Retry Job',
			'default',
			() => performRetryJob(jobId)
		);
	}

	async function performRetryJob(jobId: number) {
		activeJobId = jobId;
		error = '';
		const response = await apiFetch('/api/jobs/retry', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ job_id: jobId })
		});

		if (!response.ok) {
			error = await response.text();
			activeJobId = null;
			return;
		}

		await loadJobs();
		notice = `Retry started for job #${jobId}.`;
		markStageIncomplete('verify');
		activeJobId = null;
	}

	function rollbackBulkApplyJob(job: JobRecord) {
		const operationIds = extractOperationIdsFromResult(job.result_json);
		if (operationIds.length === 0) {
			error = 'No rollback operation IDs were found in this job result.';
			return;
		}

		openConfirmDialog(
			`Rollback job #${job.id}?`,
			`This attempts to restore ${operationIds.length} operation(s) from the original bulk apply job.`,
			'Run Rollback',
			'danger',
			() => performRollbackBulkApplyJob(job, operationIds)
		);
	}

	async function performRollbackBulkApplyJob(job: JobRecord, operationIds: string[]) {
		activeJobId = job.id;
		error = '';
		const response = await apiFetch('/api/bulk/rollback', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ operation_ids: operationIds })
		});

		if (!response.ok) {
			error = await response.text();
			activeJobId = null;
			return;
		}

		const payload = (await response.json()) as BulkRollbackResponse;
		lastRollbackItems = payload.items;
		await loadJobs();
		notice = `Rollback complete: ok=${payload.succeeded}, fail=${payload.failed}.`;
		const sourceStage = workflowStageFromJobKind(job.kind);
		if (sourceStage && sourceStage !== 'verify') {
			markStageIncomplete(sourceStage);
		}
		markStageComplete('verify');
		activeJobId = null;
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
	<title>Media Manager | Queue</title>
</svelte:head>

<main class="queue-shell">
	<PageHero
		eyebrow="Bulk processing visibility"
		title="Queue"
		lead="Recent bulk dry-run and apply jobs with status, timestamps, and error details."
	/>

	<section class="queue-card">
		<SurfaceCard as="div">
			<SectionHeader title="Job Feed" href="/operations" label="Review Operations" />
		<div class="actions">
			<button type="button" onclick={loadJobs} disabled={loading}>Refresh</button>
			<label>
				<span>Status</span>
				<select bind:value={statusFilter} onchange={applyFilters}>
					<option value="all">All</option>
					<option value="running">Running</option>
					<option value="succeeded">Succeeded</option>
					<option value="failed">Failed</option>
					<option value="canceled">Canceled</option>
				</select>
			</label>
			<label>
				<span>Kind filter</span>
				<input bind:value={kindFilter} placeholder="bulk_apply / bulk_dry_run" onkeydown={async (event) => {
					if (event.key === 'Enter') {
						await applyFilters();
					}
				}} />
			</label>
			<button type="button" onclick={applyFilters} disabled={loading}>Apply</button>
		</div>

		<p class="mono summary-line">
			total={totalCount} showing {jobs.length === 0 ? 0 : offset + 1}-{Math.min(offset + jobs.length, totalCount)} offset={offset} page_size={pageSize}
		</p>

		<OperationResultBanner
			notice={notice}
			error={jobs.length > 0 ? error : ''}
			nextHref="/operations"
			nextLabel="Next: Review Operations"
		/>

		{#if lastRollbackItems.length > 0}
			<ul class="rows mono rollback-audit-list">
				{#each lastRollbackItems as item}
					<li>
						<span>{item.operation_id}</span>
						<strong>{item.success ? item.detail ?? 'restored' : item.error ?? 'failed'}</strong>
					</li>
				{/each}
			</ul>
		{/if}

		{#if loading}
			<p class="mono summary-line">Loading bulk jobs...</p>
		{:else if error}
			<p class="error">{error}</p>
		{:else if jobs.length === 0}
			<p class="mono summary-line">No jobs yet. Run indexing from Consolidation or preview/apply from Library.</p>
		{:else}
			<div class="table-wrap">
				<table>
					<thead>
						<tr>
							<th>ID</th>
							<th>Stage</th>
							<th>Kind</th>
							<th>Status</th>
							<th>Created</th>
							<th>Updated</th>
							<th>Summary</th>
							<th>Actions</th>
							<th>Details</th>
							<th>Error</th>
						</tr>
					</thead>
					<tbody>
						{#each jobs as job}
							{@const summary = parseBulkSummary(job.result_json)}
							<tr>
								<td class="mono">{job.id}</td>
								<td class="mono">{workflowLabelFromJobKind(job.kind)}</td>
								<td class="mono">{job.kind}</td>
								<td><span class={`status ${job.status}`}>{job.status}</span></td>
								<td class="mono">{new Date(job.created_at_ms).toLocaleString()}</td>
								<td class="mono">{new Date(job.updated_at_ms).toLocaleString()}</td>
								<td class="mono">
									{#if summary}
										total={summary.total} ok={summary.succeeded} fail={summary.failed}
									{:else}
										n/a
									{/if}
								</td>
								<td>
									<button type="button" disabled={loading || activeJobId === job.id || job.status !== 'running'} onclick={() => cancelJob(job.id)}>Cancel</button>
									<button type="button" disabled={loading || activeJobId === job.id || job.status === 'running'} onclick={() => retryJob(job.id)}>Retry</button>
									<button
										type="button"
										disabled={
											loading ||
											activeJobId === job.id ||
											job.kind !== 'bulk_apply' ||
											job.status !== 'succeeded' ||
											extractOperationIdsFromResult(job.result_json).length === 0
										}
										onclick={() => rollbackBulkApplyJob(job)}
									>
										Rollback
									</button>
								</td>
								<td>
									<button type="button" onclick={() => toggleDetails(job.id)}>{expandedJobIds.includes(job.id) ? 'Hide' : 'Show'}</button>
								</td>
								<td class="mono">{job.error ?? 'n/a'}</td>
							</tr>
							{#if expandedJobIds.includes(job.id)}
								<tr class="detail-row">
									<td colspan="10">
										<div class="detail-grid">
											{#if job.kind === 'bulk_rollback' && extractRollbackItems(job.result_json).length > 0}
												<section>
													<div class="detail-heading">
														<h3>Rollback Items</h3>
													</div>
													<ul class="rows mono rollback-audit-list">
														{#each extractRollbackItems(job.result_json) as item}
															<li>
																<span>{item.operation_id}</span>
																<strong>{item.success ? item.detail ?? 'restored' : item.error ?? 'failed'}</strong>
															</li>
														{/each}
													</ul>
												</section>
											{/if}
											<section>
												<div class="detail-heading">
													<h3>Payload</h3>
													<button type="button" onclick={() => copyJson(job.payload_json, 'Payload JSON')}>Copy</button>
												</div>
												<pre class="mono">{formatJson(job.payload_json)}</pre>
											</section>
											<section>
												<div class="detail-heading">
													<h3>Result</h3>
													<button type="button" onclick={() => copyJson(job.result_json, 'Result JSON')}>Copy</button>
												</div>
												<pre class="mono">{formatJson(job.result_json)}</pre>
											</section>
										</div>
									</td>
								</tr>
							{/if}
						{/each}
					</tbody>
				</table>
			</div>
			<div class="pager">
				<button type="button" onclick={previousPage} disabled={loading || offset === 0}>Previous</button>
				<button type="button" onclick={nextPage} disabled={loading || offset + pageSize >= totalCount}>Next</button>
			</div>
		{/if}
		</SurfaceCard>
	</section>

	<ConfirmDialog
		open={confirmDialog.open}
		title={confirmDialog.title}
		message={confirmDialog.message}
		confirmLabel={confirmDialog.confirmLabel}
		busy={confirmDialog.busy}
		tone={confirmDialog.tone}
		onCancel={closeConfirmDialog}
		onConfirm={runConfirmDialogAction}
	/>
</main>

<style>
	.queue-shell {
		width: min(var(--content-max), 94vw);
		margin: var(--space-4) auto calc(var(--space-6) * 2);
		display: grid;
		gap: var(--space-4);
	}

	.queue-card {
		display: grid;
		gap: var(--space-3);
		backdrop-filter: blur(2px);
	}

	.actions {
		display: flex;
		gap: var(--space-2);
		align-items: end;
		flex-wrap: wrap;
	}

	label {
		display: grid;
		gap: var(--space-1);
		min-width: 180px;
	}

	label span {
		font-size: var(--font-small);
		color: var(--muted);
	}

	button {
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		padding: 0.5rem 0.65rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
		cursor: pointer;
		font-weight: 600;
		font-size: var(--font-small);
	}

	button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	input,
	select {
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		padding: 0.45rem 0.55rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
		font-size: var(--font-small);
	}

	.summary-line {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
	}

	.table-wrap {
		overflow-x: auto;
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		background: color-mix(in srgb, var(--card) 96%, transparent);
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: var(--font-body);
	}

	th,
	td {
		padding: var(--space-2) var(--space-3);
		border-bottom: 1px solid var(--ring);
		text-align: left;
		vertical-align: top;
	}

	th {
		font-size: var(--font-label);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--muted);
	}

	.status {
		display: inline-flex;
		padding: 0.2rem 0.5rem;
		border-radius: 999px;
		font-weight: 700;
	}

	.status.running {
		background: color-mix(in srgb, var(--accent) 20%, transparent);
	}

	.status.succeeded {
		background: color-mix(in srgb, #22c55e 20%, transparent);
	}

	.status.failed {
		background: color-mix(in srgb, var(--danger) 20%, transparent);
	}

	.status.canceled {
		background: color-mix(in srgb, #f59e0b 24%, transparent);
	}

	.detail-row td {
		background: color-mix(in srgb, var(--card) 85%, transparent);
	}

	.detail-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: var(--space-3);
	}

	.detail-grid section {
		display: grid;
		gap: var(--space-2);
	}

	.detail-heading {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: var(--space-2);
	}

	.detail-grid h3 {
		margin: 0;
		font-size: var(--font-small);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--muted);
	}

	pre {
		margin: 0;
		max-height: 260px;
		overflow: auto;
		padding: var(--space-3);
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		background: color-mix(in srgb, var(--bg) 70%, transparent);
		font-size: var(--font-small);
	}

	.error {
		color: var(--danger);
		font-weight: 700;
	}

	.rollback-audit-list {
		margin-top: var(--space-3);
	}

	.pager {
		display: flex;
		justify-content: flex-end;
		gap: var(--space-3);
		margin-top: var(--space-4);
	}

	@media (max-width: 900px) {
		.queue-shell {
			width: 96vw;
		}

		.detail-grid {
			grid-template-columns: 1fr;
		}

		label,
		button {
			width: 100%;
		}
	}
</style>
