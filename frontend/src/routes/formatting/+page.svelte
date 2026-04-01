<script lang="ts">
	import { onMount } from 'svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
	import { markStageComplete, markStageIncomplete } from '$lib/workflow/progress';
	import { apiFetch } from '$lib/utils/api';
	import {
		BULK_ACTION_RENAME,
		type BulkDryRunResponse,
		type BulkApplyResponse,
		type BulkRollbackResponse
	} from '$lib/types/api';

	type FormattingCandidateItem = {
		media_path: string;
		proposed_media_path: string;
		note: string;
	};

	type FormattingCandidatesResponse = {
		total_items: number;
		items: FormattingCandidateItem[];
	};

	let loading = $state(false);
	let error = $state('');
	let notice = $state('');
	let items = $state<FormattingCandidateItem[]>([]);
	let selectedPaths = $state<string[]>([]);
	let busy = $state(false);
	let preview = $state<BulkDryRunResponse | null>(null);
	let applyResult = $state<BulkApplyResponse | null>(null);
	let rollbackResult = $state<BulkRollbackResponse | null>(null);
	let rollbackOperationIds = $state<string[]>([]);
	let renameParentFolders = $state(false);

	onMount(async () => {
		await refresh();
	});

	async function refresh() {
		loading = true;
		error = '';
		const response = await apiFetch('/api/formatting/candidates?offset=0&limit=250');
		if (!response.ok) {
			error = `Unable to load formatting candidates (${response.status})`;
			loading = false;
			return;
		}

		const payload = (await response.json()) as FormattingCandidatesResponse;
		items = payload.items;
		selectedPaths = selectedPaths.filter((path) => items.some((item) => item.media_path === path));
		preview = null;
		applyResult = null;
		rollbackResult = null;
		loading = false;
	}

	function isSelected(mediaPath: string): boolean {
		return selectedPaths.includes(mediaPath);
	}

	function toggleSelection(mediaPath: string) {
		if (isSelected(mediaPath)) {
			selectedPaths = selectedPaths.filter((path) => path !== mediaPath);
		} else {
			selectedPaths = [...selectedPaths, mediaPath];
		}
		preview = null;
	}

	function selectAll() {
		selectedPaths = items.map((item) => item.media_path);
		preview = null;
	}

	function clearSelection() {
		selectedPaths = [];
		preview = null;
	}

	function buildPayload() {
		return selectedPaths.map((mediaPath) => {
			const file = mediaPath.split('/').pop() ?? mediaPath;
			return {
				media_path: mediaPath,
				item_uid: file.replace(/\.[^.]+$/, ''),
				rename_parent_folder: renameParentFolders
			};
		});
	}

	async function runPreview() {
		if (selectedPaths.length === 0) {
			error = 'Select at least one candidate first.';
			return;
		}

		busy = true;
		error = '';
		notice = '';
		applyResult = null;
		rollbackResult = null;
		const response = await apiFetch('/api/bulk/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: BULK_ACTION_RENAME,
				items: buildPayload()
			})
		});
		if (!response.ok) {
			error = await response.text();
			busy = false;
			return;
		}

		preview = (await response.json()) as BulkDryRunResponse;
		busy = false;
	}

	async function applyPreview() {
		if (!preview) {
			error = 'Run preview before apply.';
			return;
		}
		if (!preview.plan_ready) {
			error = 'Preview includes invalid items. Resolve them before apply.';
			return;
		}

		busy = true;
		error = '';
		notice = '';
		rollbackResult = null;
		const response = await apiFetch('/api/bulk/apply', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: BULK_ACTION_RENAME,
				approved_batch_hash: preview.batch_hash,
				items: buildPayload()
			})
		});
		if (!response.ok) {
			error = await response.text();
			busy = false;
			return;
		}

		applyResult = (await response.json()) as BulkApplyResponse;
		rollbackOperationIds = applyResult.items
			.filter((item) => item.success && !!item.operation_id)
			.map((item) => item.operation_id as string);
		notice = `Formatting apply complete: ok=${applyResult.succeeded}, fail=${applyResult.failed}. Rollback ready=${rollbackOperationIds.length}.`;
		if (applyResult.failed === 0) {
			markStageComplete('formatting');
		}
		busy = false;
		await refresh();
	}

	async function rollbackLastApply() {
		if (rollbackOperationIds.length === 0) {
			error = 'No rollback operations are available from the last apply.';
			return;
		}

		busy = true;
		error = '';
		notice = '';
		const response = await apiFetch('/api/bulk/rollback', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ operation_ids: rollbackOperationIds })
		});
		if (!response.ok) {
			error = await response.text();
			busy = false;
			return;
		}

		rollbackResult = (await response.json()) as BulkRollbackResponse;
		notice = `Rollback complete: ok=${rollbackResult.succeeded}, fail=${rollbackResult.failed}.`;
		if (rollbackResult.failed === 0) {
			rollbackOperationIds = [];
			markStageIncomplete('formatting');
		}
		busy = false;
		await refresh();
	}

</script>

<svelte:head>
	<title>Media Manager | Formatting</title>
</svelte:head>

<main class="stage-shell">
	<PageHero
		eyebrow="Stage 3"
		title="Formatting"
		lead="Review deterministic rename candidates generated from the indexed library snapshot before applying formatting operations."
	>
		<p class="mono policy">
			Default rename policy: <strong>Movie Name - Subtitle (Year)</strong>. Spaces are preserved; only invalid filename
			characters are replaced with <code>-</code>. Matching sidecar <code>.nfo</code> files remain aligned automatically.
		</p>
	</PageHero>

	<section class="stage-card">
		<SurfaceCard as="div">
			<div class="actions">
			<button type="button" onclick={refresh} disabled={loading}>Refresh</button>
			<button type="button" onclick={selectAll} disabled={loading || items.length === 0}>Select All</button>
			<button type="button" onclick={clearSelection} disabled={loading || selectedPaths.length === 0}>Clear</button>
			<button type="button" onclick={runPreview} disabled={busy || selectedPaths.length === 0}>Preview Rename</button>
			<button type="button" onclick={applyPreview} disabled={busy || !preview || !preview.plan_ready}>Apply Rename</button>
			<button type="button" onclick={rollbackLastApply} disabled={busy || rollbackOperationIds.length === 0}>Rollback Last Apply</button>
			<a class="library-link" href="/library">Open Library Rename/NFO Actions</a>
			<label class="toggle mono">
				<input
					type="checkbox"
					bind:checked={renameParentFolders}
					onchange={() => {
						preview = null;
					}}
				/>
				Rename parent folders (grouped/multi-file folders only)
			</label>
			</div>
			<OperationResultBanner notice={notice} error={error} nextHref="/queue" nextLabel="Next: Verify In Queue" />

			<p class="mono summary-line">selected={selectedPaths.length}</p>
			{#if preview}
				<p class="mono summary-line">preview batch={preview.batch_hash} total={preview.total_items} invalid={preview.summary.invalid}</p>
			{/if}
			{#if applyResult}
				<p class="mono summary-line">applied total={applyResult.total_items} ok={applyResult.succeeded} fail={applyResult.failed}</p>
			{/if}
			{#if rollbackResult}
				<p class="mono summary-line">rollback total={rollbackResult.total_items} ok={rollbackResult.succeeded} fail={rollbackResult.failed}</p>
			{/if}

			{#if loading}
				<p class="mono summary-line">Loading formatting candidates...</p>
			{:else if items.length === 0}
				<p class="mono summary-line">No rename candidates found in current indexed window.</p>
			{:else}
				<div class="table-wrap">
					<table class="mono">
						<thead>
							<tr>
								<th>Select</th>
								<th>Current Path</th>
								<th>Proposed Path</th>
								<th>Note</th>
							</tr>
						</thead>
						<tbody>
							{#each items as item}
								<tr>
									<td><input type="checkbox" checked={isSelected(item.media_path)} onchange={() => toggleSelection(item.media_path)} /></td>
									<td>{item.media_path}</td>
									<td>{item.proposed_media_path}</td>
									<td>{item.note}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</SurfaceCard>
	</section>
</main>

<style>
	.stage-shell {
		width: min(var(--content-max), 94vw);
		margin: var(--space-4) auto calc(var(--space-6) * 2);
		display: grid;
		gap: var(--space-4);
	}

	.policy {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
		line-height: 1.4;
	}

	.policy code {
		font-size: var(--font-tiny);
	}

	.stage-card {
		display: grid;
		gap: var(--space-3);
		backdrop-filter: blur(2px);
	}

	.actions {
		display: flex;
		gap: var(--space-2);
		align-items: center;
		flex-wrap: wrap;
	}

	button,
	.library-link {
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		padding: 0.45rem 0.6rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
		font-weight: 700;
		text-decoration: none;
		font-size: var(--font-small);
	}

	button {
		cursor: pointer;
	}

	button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	.toggle {
		display: inline-flex;
		align-items: center;
		gap: var(--space-2);
		font-size: var(--font-small);
		color: var(--muted);
	}

	.summary-line {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
	}

	.table-wrap {
		overflow-x: auto;
		margin-top: var(--space-2);
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		background: color-mix(in srgb, var(--card) 95%, transparent);
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

	tbody tr:last-child td {
		border-bottom: none;
	}

	@media (max-width: 760px) {
		.actions {
			gap: var(--space-2);
		}

		button,
		.library-link {
			width: 100%;
			text-align: center;
		}

		table {
			font-size: var(--font-small);
		}
	}

</style>
