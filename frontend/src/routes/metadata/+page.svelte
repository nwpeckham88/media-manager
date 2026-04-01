<script lang="ts">
	import { onMount } from 'svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SectionHeader from '$lib/components/ui/SectionHeader.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
	import { markStageComplete, markStageIncomplete } from '$lib/workflow/progress';
	import { apiFetch } from '$lib/utils/api';
	import type { BulkDryRunResponse, BulkApplyResponse, BulkRollbackResponse } from '$lib/types/api';

	type IndexedMediaItem = {
		media_path: string;
		parsed_title: string | null;
		parsed_year: number | null;
		parsed_provider_id: string | null;
		metadata_confidence: number | null;
	};

	type IndexItemsResponse = {
		total_items: number;
		items: IndexedMediaItem[];
	};

	type BulkAction = 'metadata_lookup';

	let loading = $state(false);
	let error = $state('');
	let notice = $state('');
	let query = $state('');
	let onlyMissingProvider = $state(true);
	let maxConfidence = $state(0.95);
	let items = $state<IndexedMediaItem[]>([]);
	let selectedPaths = $state<string[]>([]);
	let preview = $state<BulkDryRunResponse | null>(null);
	let applyResult = $state<BulkApplyResponse | null>(null);
	let rollbackResult = $state<BulkRollbackResponse | null>(null);
	let rollbackOperationIds = $state<string[]>([]);
	let busy = $state(false);

	onMount(async () => {
		await refresh();
	});

	async function refresh() {
		loading = true;
		error = '';
		const params = new URLSearchParams();
		params.set('limit', '250');
		params.set('offset', '0');
		if (query.trim().length > 0) {
			params.set('q', query.trim());
		}
		if (onlyMissingProvider) {
			params.set('only_missing_provider', 'true');
		}
		params.set('max_confidence', String(maxConfidence));

		const response = await apiFetch(`/api/index/items?${params.toString()}`);
		if (!response.ok) {
			error = `Unable to load metadata candidates (${response.status})`;
			loading = false;
			return;
		}

		const payload = (await response.json()) as IndexItemsResponse;
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

	function deriveUid(item: IndexedMediaItem): string {
		const fromTitle = (item.parsed_title ?? '').trim();
		if (fromTitle.length > 0) {
			const yearSuffix = item.parsed_year ? `-${item.parsed_year}` : '';
			return `${fromTitle.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')}${yearSuffix}`;
		}
		const file = item.media_path.split('/').pop() ?? item.media_path;
		return file.replace(/\.[^.]+$/, '');
	}

	function buildBulkItemsPayload() {
		return selectedPaths.map((mediaPath) => {
			const item = items.find((value) => value.media_path === mediaPath);
			if (!item) {
				return {
					media_path: mediaPath,
					item_uid: mediaPath,
					metadata_override: undefined
				};
			}

			return {
				media_path: item.media_path,
				item_uid: deriveUid(item),
				metadata_override: {
					title: item.parsed_title,
					year: item.parsed_year,
					provider_id: item.parsed_provider_id,
					confidence: item.metadata_confidence
				}
			};
		});
	}

	async function runPreview() {
		if (selectedPaths.length === 0) {
			error = 'Select at least one item first.';
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
				action: 'metadata_lookup',
				items: buildBulkItemsPayload()
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
				action: 'metadata_lookup',
				approved_batch_hash: preview.batch_hash,
				items: buildBulkItemsPayload()
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
		notice = `Metadata apply complete: ok=${applyResult.succeeded}, fail=${applyResult.failed}. Rollback ready=${rollbackOperationIds.length}.`;
		if (applyResult.failed === 0) {
			markStageComplete('metadata');
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
			markStageIncomplete('metadata');
		}
		busy = false;
		await refresh();
	}

</script>

<svelte:head>
	<title>Media Manager | Metadata</title>
</svelte:head>

<main class="stage-shell">
	<PageHero
		eyebrow="Stage 2"
		title="Metadata"
		lead="Review parser output and focus on items missing provider IDs or with lower-confidence metadata inference."
	/>

	<section class="stage-card">
		<SurfaceCard as="div">
			<SectionHeader title="Candidate Review" href="/library" label="Open Library Bulk Editor" />
		<div class="actions">
			<input bind:value={query} placeholder="search title/path/provider" />
			<label class="toggle mono"><input type="checkbox" bind:checked={onlyMissingProvider} /> Only Missing Provider</label>
			<label class="mono">Max Confidence <input class="conf" type="number" min="0" max="1" step="0.01" bind:value={maxConfidence} /></label>
			<button type="button" onclick={refresh} disabled={loading}>Refresh</button>
			<button type="button" onclick={selectAll} disabled={loading || items.length === 0}>Select All</button>
			<button type="button" onclick={clearSelection} disabled={loading || selectedPaths.length === 0}>Clear</button>
			<button type="button" onclick={runPreview} disabled={busy || selectedPaths.length === 0}>Preview Apply</button>
			<button type="button" onclick={applyPreview} disabled={busy || !preview || !preview.plan_ready}>Apply Preview</button>
			<button type="button" onclick={rollbackLastApply} disabled={busy || rollbackOperationIds.length === 0}>Rollback Last Apply</button>
		</div>
		<OperationResultBanner notice={notice} error={error} nextHref="/formatting" nextLabel="Next: Formatting" />

		<p class="mono summary-line">selected={selectedPaths.length}</p>
		{#if preview}
			<p class="mono summary-line">preview batch={preview.batch_hash} total={preview.total_items} creates={preview.summary.creates} updates={preview.summary.updates} invalid={preview.summary.invalid}</p>
		{/if}
		{#if applyResult}
			<p class="mono summary-line">applied total={applyResult.total_items} ok={applyResult.succeeded} fail={applyResult.failed}</p>
		{/if}
		{#if rollbackResult}
			<p class="mono summary-line">rollback total={rollbackResult.total_items} ok={rollbackResult.succeeded} fail={rollbackResult.failed}</p>
		{/if}

		{#if loading}
			<p class="mono summary-line">Loading metadata candidates...</p>
		{:else if items.length === 0}
			<p class="mono summary-line">No candidates matched current filters.</p>
		{:else}
			<div class="table-wrap">
				<table class="mono">
					<thead>
						<tr>
							<th>Select</th>
							<th>Media Path</th>
							<th>Parsed Title</th>
							<th>Year</th>
							<th>Provider</th>
							<th>Confidence</th>
						</tr>
					</thead>
					<tbody>
						{#each items as item}
							<tr>
								<td><input type="checkbox" checked={isSelected(item.media_path)} onchange={() => toggleSelection(item.media_path)} /></td>
								<td>{item.media_path}</td>
								<td>{item.parsed_title ?? 'n/a'}</td>
								<td>{item.parsed_year ?? 'n/a'}</td>
								<td>{item.parsed_provider_id ?? 'missing'}</td>
								<td>{item.metadata_confidence !== null ? item.metadata_confidence.toFixed(2) : 'n/a'}</td>
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

	input,
	button {
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		padding: 0.45rem 0.6rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
		font-size: var(--font-small);
	}

	button {
		font-weight: 700;
		cursor: pointer;
	}

	button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	.summary-line {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
	}

	input {
		min-width: 15rem;
	}

	.table-wrap {
		overflow-x: auto;
		margin-top: var(--space-2);
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

	.toggle {
		display: inline-flex;
		align-items: center;
		gap: var(--space-2);
		font-size: var(--font-small);
	}

	.conf {
		width: 6rem;
		min-width: 0;
	}

	@media (max-width: 760px) {
		input,
		button {
			width: 100%;
		}

		.conf {
			width: 100%;
		}
	}

</style>
