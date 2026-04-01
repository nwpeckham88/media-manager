<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
	import SectionHeader from '$lib/components/ui/SectionHeader.svelte';
	import { apiFetch } from '$lib/utils/api';
	import {
		BULK_ACTION_RENAME,
		type BulkAction,
		type BulkDryRunResponse,
		type BulkApplyResponse,
		type ConfirmDialogState
	} from '$lib/types/api';

	type AppConfig = {
		library_roots: string[];
		auth_enabled: boolean;
	};

	type LibraryItem = {
		media_path: string;
		root: string;
		relative_path: string;
		file_name: string;
		extension: string;
		sidecar_path: string;
		sidecar_exists: boolean;
	};

	type LibraryBrowseResult = {
		total_matches: number;
		offset: number;
		limit: number;
		items: LibraryItem[];
	};

	type MetadataOverrideDraft = {
		title: string;
		year: string;
		providerId: string;
		confidence: string;
	};

	let config = $state<AppConfig | null>(null);
	let items = $state<LibraryItem[]>([]);
	let totalMatches = $state(0);
	let loading = $state(false);
	let error = $state('');
	let rootIndex = $state<number | null>(null);
	let searchQuery = $state('');
	let offset = $state(0);
	let pageSize = $state(120);
	let selectedPaths = $state<string[]>([]);
	let bulkAction = $state<BulkAction>('metadata_lookup');
	let renameParentFolders = $state(false);
	let preview = $state<BulkDryRunResponse | null>(null);
	let applyResult = $state<BulkApplyResponse | null>(null);
	let bulkBusy = $state(false);
	let metadataOverrides = $state<Record<string, MetadataOverrideDraft>>({});
	let metadataPreviewStale = $state(false);
	let confirmDialog = $state<ConfirmDialogState>({
		open: false,
		title: '',
		message: '',
		confirmLabel: 'Confirm',
		busy: false
	});
	let pendingConfirmAction = $state<null | (() => Promise<void>)>(null);

	onMount(async () => {
		await loadConfig();
		await loadItems();
	});

	async function loadConfig() {
		const response = await apiFetch('/api/config/app');
		if (!response.ok) {
			error = `Unable to load app config (${response.status})`;
			return;
		}

		config = (await response.json()) as AppConfig;
	}

	async function loadItems() {
		loading = true;
		error = '';

		const params = new URLSearchParams();
		params.set('offset', String(offset));
		params.set('limit', String(pageSize));
		if (rootIndex !== null) {
			params.set('root_index', String(rootIndex));
		}
		if (searchQuery.trim().length > 0) {
			params.set('q', searchQuery.trim());
		}

		const response = await apiFetch(`/api/library/items?${params.toString()}`);
		if (!response.ok) {
			error = await response.text();
			loading = false;
			return;
		}

		const payload = (await response.json()) as LibraryBrowseResult;
		items = payload.items;
		totalMatches = payload.total_matches;
		selectedPaths = selectedPaths.filter((path) => payload.items.some((item) => item.media_path === path));
		preview = null;
		applyResult = null;
		loading = false;
	}

	function isSelected(mediaPath: string): boolean {
		return selectedPaths.includes(mediaPath);
	}

	function toggleSelection(mediaPath: string) {
		if (isSelected(mediaPath)) {
			selectedPaths = selectedPaths.filter((path) => path !== mediaPath);
			return;
		}

		selectedPaths = [...selectedPaths, mediaPath];
	}

	function selectAllPage() {
		selectedPaths = [...new Set([...selectedPaths, ...items.map((item) => item.media_path)])];
	}

	function clearSelection() {
		selectedPaths = [];
	}

	async function applyFilter() {
		offset = 0;
		await loadItems();
	}

	async function nextPage() {
		if (offset + pageSize >= totalMatches) {
			return;
		}

		offset += pageSize;
		await loadItems();
	}

	async function previousPage() {
		if (offset === 0) {
			return;
		}

		offset = Math.max(0, offset - pageSize);
		await loadItems();
	}

	function buildBulkItemsPayload() {
		return selectedPaths.map((mediaPath) => {
			const item = items.find((value) => value.media_path === mediaPath);
			const fallbackUid = item?.file_name.replace(/\.[^.]+$/, '') ?? mediaPath;
			const metadataOverride = bulkAction === 'metadata_lookup' ? buildMetadataOverridePayload(mediaPath) : undefined;
			return {
				media_path: mediaPath,
				item_uid: fallbackUid,
				metadata_override: metadataOverride,
				rename_parent_folder: bulkAction === BULK_ACTION_RENAME ? renameParentFolders : undefined
			};
		});
	}

	function buildMetadataOverridePayload(mediaPath: string) {
		const draft = metadataOverrides[mediaPath];
		if (!draft) {
			return undefined;
		}

		const title = draft.title.trim();
		const providerId = draft.providerId.trim();
		const yearValue = draft.year.trim();
		const confidenceValue = draft.confidence.trim();
		const year = yearValue.length > 0 ? Number.parseInt(yearValue, 10) : Number.NaN;
		const confidence = confidenceValue.length > 0 ? Number.parseFloat(confidenceValue) : Number.NaN;

		return {
			title: title.length > 0 ? title : null,
			year: Number.isFinite(year) && year >= 1900 && year <= 2100 ? year : null,
			provider_id: providerId.length > 0 ? providerId : null,
			confidence: Number.isFinite(confidence) ? Math.max(0, Math.min(1, confidence)) : null
		};
	}

	function initializeMetadataOverrides(itemsForPreview: NonNullable<BulkDryRunResponse['items']>) {
		const next: Record<string, MetadataOverrideDraft> = {};
		for (const item of itemsForPreview) {
			next[item.media_path] = {
				title: item.metadata_title ?? item.item_uid ?? '',
				year: item.metadata_year != null ? String(item.metadata_year) : '',
				providerId: item.proposed_provider_id ?? '',
				confidence: item.metadata_confidence != null ? item.metadata_confidence.toFixed(2) : ''
			};
		}

		metadataOverrides = next;
	}

	function updateMetadataDraft(mediaPath: string, field: keyof MetadataOverrideDraft, value: string) {
		const existing = metadataOverrides[mediaPath] ?? {
			title: '',
			year: '',
			providerId: '',
			confidence: ''
		};

		metadataOverrides = {
			...metadataOverrides,
			[mediaPath]: {
				...existing,
				[field]: value
			}
		};

		if (preview?.action === 'metadata_lookup') {
			metadataPreviewStale = true;
		}
	}

	async function runBulkPreview() {
		if (selectedPaths.length === 0) {
			error = 'Select at least one item first.';
			return;
		}

		bulkBusy = true;
		error = '';
		applyResult = null;

		const response = await apiFetch('/api/bulk/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: bulkAction,
				items: buildBulkItemsPayload()
			})
		});
		if (!response.ok) {
			error = await response.text();
			bulkBusy = false;
			return;
		}

		preview = (await response.json()) as BulkDryRunResponse;
		if (preview.action === 'metadata_lookup') {
			initializeMetadataOverrides(preview.items ?? []);
			metadataPreviewStale = false;
		}
		bulkBusy = false;
	}

	function openConfirmDialog(title: string, message: string, confirmLabel: string, action: () => Promise<void>) {
		confirmDialog = {
			open: true,
			title,
			message,
			confirmLabel,
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

	function applyBulkPreview() {
		if (!preview) {
			error = 'Run preview before apply.';
			return;
		}
		if (!preview.plan_ready) {
			error = 'Preview includes invalid items; fix those before apply.';
			return;
		}
		if (preview.action === 'metadata_lookup' && metadataPreviewStale) {
			error = 'Metadata edits changed since last preview. Run preview again before apply.';
			return;
		}

		openConfirmDialog(
			`Apply ${preview.total_items} item(s)?`,
			`This runs ${preview.action} with the currently approved batch hash.`,
			'Apply Preview',
			performApplyBulkPreview
		);
	}

	async function performApplyBulkPreview() {
		if (!preview) {
			error = 'Preview no longer available.';
			return;
		}

		bulkBusy = true;
		error = '';

		const response = await apiFetch('/api/bulk/apply', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: bulkAction,
				approved_batch_hash: preview.batch_hash,
				items: buildBulkItemsPayload()
			})
		});
		if (!response.ok) {
			error = await response.text();
			bulkBusy = false;
			return;
		}

		applyResult = (await response.json()) as BulkApplyResponse;
		metadataPreviewStale = false;
		await loadItems();
		bulkBusy = false;
	}

</script>

<svelte:head>
	<title>Media Manager | Library</title>
</svelte:head>

<main class="library-shell">
	<PageHero
		eyebrow="Browse-first management"
		title="Library"
		lead="Advanced utility for manual inspection and targeted fixes. For normal run order use Consolidation -> Metadata -> Formatting first, then return here for exceptions."
	>
		<p class="mono helper-links"><a href="/">Open Workflow Dashboard</a> | <a href="/operations">Open Operations</a></p>
	</PageHero>

	<section class="stage-card controls">
		<SurfaceCard as="div">
			<SectionHeader title="Browse and Bulk Actions" />
			<div class="control-row">
			<label>
				<span>Configured Library</span>
				<select
					value={rootIndex === null ? 'all' : String(rootIndex)}
					onchange={async (event) => {
						const value = (event.currentTarget as HTMLSelectElement).value;
						rootIndex = value === 'all' ? null : Number.parseInt(value, 10);
						offset = 0;
						await loadItems();
					}}
				>
					<option value="all">All configured roots</option>
					{#if config}
						{#each config.library_roots as root, idx}
							<option value={idx}>{root}</option>
						{/each}
					{/if}
				</select>
			</label>
			<label>
				<span>Search</span>
				<input bind:value={searchQuery} placeholder="filename or relative path" onkeydown={async (event) => {
					if (event.key === 'Enter') {
						await applyFilter();
					}
				}} />
			</label>
			<button type="button" onclick={applyFilter}>Apply Filter</button>
			<button type="button" onclick={loadItems}>Refresh</button>
			</div>

			<div class="control-row stat-row mono">
				<span>Total matches: {totalMatches}</span>
				<span>Showing: {items.length}</span>
				<span>Offset: {offset}</span>
				<span>Page size: {pageSize}</span>
			</div>

			<div class="control-row">
				<button type="button" onclick={selectAllPage}>Select Page</button>
				<button type="button" onclick={clearSelection}>Clear Selection</button>
				<span class="selection-pill mono">Selected: {selectedPaths.length}</span>
				<label class="bulk-action-label">
					<span>Bulk Action</span>
					<select bind:value={bulkAction}>
						<option value="metadata_lookup">Metadata Lookup</option>
						<option value="combine_duplicates">Combine Duplicates</option>
						<option value="rename">Rename</option>
						<option value="validate_nfo">Validate NFO</option>
					</select>
				</label>
				<button type="button" disabled={bulkBusy} onclick={runBulkPreview}>Preview Selected</button>
				<button type="button" disabled={bulkBusy || !preview || !preview.plan_ready || (preview.action === 'metadata_lookup' && metadataPreviewStale)} onclick={applyBulkPreview}>Apply Preview</button>
				<a class="queue-link" href="/queue">Queue</a>
				{#if bulkAction === BULK_ACTION_RENAME}
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
				{/if}
			</div>
		</SurfaceCard>
	</section>

	{#if preview}
		<section class="stage-card">
			<SurfaceCard as="div">
				<SectionHeader title="Bulk Preview" />
				<p class="mono summary-line">
				action={preview.action} batch={preview.batch_hash} creates={preview.summary.creates} updates={preview.summary.updates} noops={preview.summary.noops} invalid={preview.summary.invalid}
				</p>
			{#if preview.action === BULK_ACTION_RENAME}
				<ul class="rows mono">
					{#each (preview.items ?? []) as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.proposed_media_path ?? item.error ?? item.note ?? 'n/a'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if preview.action === 'validate_nfo'}
				<ul class="rows mono">
					{#each (preview.items ?? []) as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.note ? `${item.proposed_media_path ?? 'nfo'} | ${item.note}` : item.error ?? 'n/a'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if preview.action === 'combine_duplicates'}
				<ul class="rows mono">
					{#each (preview.items ?? []) as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.proposed_item_uid ?? item.item_uid} {item.note ? `| ${item.note}` : ''}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if preview.action === 'metadata_lookup'}
				<p class="mono">Edit metadata fields below, then run <strong>Preview Selected</strong> again to lock in overrides before apply.</p>
				{#if metadataPreviewStale}
					<p class="error">Metadata values changed. Preview is stale until you run Preview Selected again.</p>
				{/if}
				<div class="table-wrap">
					<table class="metadata-table mono">
						<thead>
							<tr>
								<th>Path</th>
								<th>Title</th>
								<th>Year</th>
								<th>Provider ID</th>
								<th>Confidence</th>
							</tr>
						</thead>
						<tbody>
							{#each (preview.items ?? []) as item}
								<tr>
									<td>{item.media_path}</td>
									<td>
										<input
											value={metadataOverrides[item.media_path]?.title ?? item.metadata_title ?? item.item_uid}
											oninput={(event) => updateMetadataDraft(item.media_path, 'title', (event.currentTarget as HTMLInputElement).value)}
										/>
									</td>
									<td>
										<input
											class="year-input"
											inputmode="numeric"
											value={metadataOverrides[item.media_path]?.year ?? (item.metadata_year != null ? String(item.metadata_year) : '')}
											oninput={(event) => updateMetadataDraft(item.media_path, 'year', (event.currentTarget as HTMLInputElement).value)}
										/>
									</td>
									<td>
										<input
											value={metadataOverrides[item.media_path]?.providerId ?? item.proposed_provider_id ?? ''}
											oninput={(event) => updateMetadataDraft(item.media_path, 'providerId', (event.currentTarget as HTMLInputElement).value)}
										/>
									</td>
									<td>
										<input
											class="confidence-input"
											inputmode="decimal"
											value={metadataOverrides[item.media_path]?.confidence ?? (item.metadata_confidence != null ? item.metadata_confidence.toFixed(2) : '')}
											oninput={(event) => updateMetadataDraft(item.media_path, 'confidence', (event.currentTarget as HTMLInputElement).value)}
										/>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
			{#if (preview.items ?? []).some((item) => item.error)}
				<ul class="rows mono">
					{#each (preview.items ?? []).filter((item) => item.error) as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.error}</strong>
						</li>
					{/each}
				</ul>
			{/if}
				</SurfaceCard>
		</section>
	{/if}

	{#if applyResult}
			<section class="stage-card">
				<SurfaceCard as="div">
					<SectionHeader title="Bulk Apply Result" />
					<p class="mono summary-line">
				total={applyResult.total_items} succeeded={applyResult.succeeded} failed={applyResult.failed}
					</p>
			{#if applyResult.action === BULK_ACTION_RENAME}
				<ul class="rows mono">
					{#each applyResult.items as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.final_media_path ?? item.error ?? 'unchanged'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if applyResult.action === 'validate_nfo'}
				<ul class="rows mono">
					{#each applyResult.items as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.final_media_path ?? item.error ?? 'nfo unchanged'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if applyResult.action === 'combine_duplicates'}
				<ul class="rows mono">
					{#each applyResult.items as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.success ? `uid=${item.item_uid}` : item.error ?? 'failed'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if applyResult.action === 'metadata_lookup'}
				<ul class="rows mono">
					{#each applyResult.items as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.success ? `provider=${item.applied_provider_id ?? 'none'}` : item.error ?? 'failed'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if applyResult.failed > 0}
				<ul class="rows mono">
					{#each applyResult.items.filter((item) => !item.success) as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.error ?? 'failed'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
				</SurfaceCard>
		</section>
	{/if}

		<section class="stage-card table-card">
			<SurfaceCard as="div">
				<SectionHeader title="Library Items" />
		{#if loading}
				<p class="mono summary-line">Loading library items...</p>
		{:else if error}
			<p class="error">{error}</p>
		{:else if items.length === 0}
				<p class="mono summary-line">No media items found for the current filters.</p>
		{:else}
			<div class="table-wrap">
				<table>
					<thead>
						<tr>
							<th>Select</th>
							<th>File</th>
							<th>Relative Path</th>
							<th>Root</th>
							<th>Sidecar</th>
						</tr>
					</thead>
					<tbody>
						{#each items as item}
							<tr>
								<td>
									<input
										type="checkbox"
										checked={isSelected(item.media_path)}
										onchange={() => toggleSelection(item.media_path)}
									/>
								</td>
								<td class="mono">
									<a class="item-link" href={`/library/item?media_path=${encodeURIComponent(item.media_path)}`}>{item.file_name}</a>
								</td>
								<td class="mono">{item.relative_path}</td>
								<td class="mono">{item.root}</td>
								<td>
									<span class={item.sidecar_exists ? 'badge ok' : 'badge'}>{item.sidecar_exists ? 'exists' : 'missing'}</span>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}

		<div class="pager">
			<button type="button" onclick={previousPage} disabled={loading || offset === 0}>Previous</button>
			<button type="button" onclick={nextPage} disabled={loading || offset + pageSize >= totalMatches}>Next</button>
		</div>
		</SurfaceCard>
	</section>

	<ConfirmDialog
		open={confirmDialog.open}
		title={confirmDialog.title}
		message={confirmDialog.message}
		confirmLabel={confirmDialog.confirmLabel}
		busy={confirmDialog.busy}
		tone="danger"
		onCancel={closeConfirmDialog}
		onConfirm={runConfirmDialogAction}
	/>
</main>

<style>
	.library-shell {
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

	.control-row {
		display: flex;
		gap: var(--space-2);
		align-items: end;
		flex-wrap: wrap;
	}

	.control-row + .control-row {
		margin-top: var(--space-3);
	}

	label {
		display: grid;
		gap: var(--space-1);
		min-width: 240px;
	}

	.bulk-action-label {
		min-width: 220px;
	}

	label span {
		font-size: var(--font-small);
		color: var(--muted);
	}

	input,
	select,
	button {
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		padding: 0.5rem 0.65rem;
		font: inherit;
		font-size: var(--font-small);
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
	}

	button {
		cursor: pointer;
		font-weight: 600;
	}

	button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	.selection-pill {
		padding: 0.5rem 0.7rem;
		border-radius: 999px;
		border: 1px solid var(--ring);
		font-size: var(--font-small);
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
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		background: color-mix(in srgb, var(--card) 96%, transparent);
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: var(--font-body);
	}

	.metadata-table input {
		min-width: 10rem;
		width: 100%;
	}

	.metadata-table .year-input {
		min-width: 5.5rem;
	}

	.metadata-table .confidence-input {
		min-width: 6.5rem;
	}

	th,
	td {
		padding: var(--space-2) var(--space-3);
		border-bottom: 1px solid var(--ring);
		text-align: left;
		vertical-align: top;
	}

	.helper-links {
		margin: 0;
		font-size: var(--font-small);
	}

	th {
		font-size: var(--font-label);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--muted);
	}

	.badge {
		display: inline-flex;
		padding: 0.2rem 0.5rem;
		border-radius: 999px;
		background: color-mix(in srgb, var(--danger) 12%, transparent);
		font-size: var(--font-small);
		font-weight: 700;
	}

	.badge.ok {
		background: color-mix(in srgb, var(--accent) 22%, transparent);
	}

	.pager {
		display: flex;
		gap: var(--space-3);
		justify-content: flex-end;
		margin-top: var(--space-4);
	}

	.error {
		color: var(--danger);
		font-weight: 700;
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
		gap: var(--space-3);
		border-bottom: 1px dashed var(--ring);
		padding-bottom: var(--space-2);
	}

	.queue-link {
		display: inline-flex;
		align-items: center;
		padding: 0.5rem 0.75rem;
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		text-decoration: none;
		font-weight: 700;
		font-size: var(--font-small);
	}

	.item-link {
		text-decoration: none;
		font-weight: 700;
		border-bottom: 1px dotted var(--ring);
	}

	@media (max-width: 900px) {
		.library-shell {
			width: 96vw;
		}

		label {
			min-width: 180px;
		}

		button,
		.queue-link {
			width: 100%;
			justify-content: center;
		}

		.rows li {
			flex-direction: column;
		}
	}
</style>
