<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';

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

	type BulkAction = 'metadata_lookup' | 'combine_duplicates' | 'rename' | 'validate_nfo';

	type MetadataOverrideDraft = {
		title: string;
		year: string;
		providerId: string;
		confidence: string;
	};

	type BulkDryRunResponse = {
		action: BulkAction;
		batch_hash: string;
		total_items: number;
		plan_ready: boolean;
		summary: {
			creates: number;
			updates: number;
			noops: number;
			invalid: number;
		};
		items: Array<{
			media_path: string;
			item_uid: string;
			plan: {
				plan_hash: string;
				action: 'create' | 'update' | 'noop';
				sidecar_path: string;
			} | null;
			proposed_media_path: string | null;
			proposed_item_uid: string | null;
			proposed_provider_id: string | null;
			metadata_title: string | null;
			metadata_year: number | null;
			metadata_confidence: number | null;
			can_apply: boolean;
			note: string | null;
			error: string | null;
		}>;
	};

	type BulkApplyResponse = {
		action: BulkAction;
		batch_hash: string;
		total_items: number;
		succeeded: number;
		failed: number;
		items: Array<{
			media_path: string;
			final_media_path: string | null;
			item_uid: string;
			applied_provider_id: string | null;
			success: boolean;
			operation_id: string | null;
			error: string | null;
		}>;
	};

	type ConfirmDialogState = {
		open: boolean;
		title: string;
		message: string;
		confirmLabel: string;
		busy: boolean;
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
				metadata_override: metadataOverride
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

	function initializeMetadataOverrides(itemsForPreview: BulkDryRunResponse['items']) {
		const next: Record<string, MetadataOverrideDraft> = {};
		for (const item of itemsForPreview) {
			next[item.media_path] = {
				title: item.metadata_title ?? item.item_uid,
				year: item.metadata_year !== null ? String(item.metadata_year) : '',
				providerId: item.proposed_provider_id ?? '',
				confidence: item.metadata_confidence !== null ? item.metadata_confidence.toFixed(2) : ''
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
			initializeMetadataOverrides(preview.items);
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
	<title>Media Manager | Library</title>
</svelte:head>

<main class="library-shell">
	<section class="library-hero">
		<p class="eyebrow">Browse-first management</p>
		<h1>Library</h1>
		<p class="lead">Browse all configured roots, search quickly, and stage selections for bulk metadata, dedupe, rename, and NFO workflows.</p>
	</section>

	<section class="controls card">
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
		</div>
	</section>

	{#if preview}
		<section class="card">
			<h2>Bulk Preview</h2>
			<p class="mono">
				action={preview.action} batch={preview.batch_hash} creates={preview.summary.creates} updates={preview.summary.updates} noops={preview.summary.noops} invalid={preview.summary.invalid}
			</p>
			{#if preview.action === 'rename'}
				<ul class="rows mono">
					{#each preview.items as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.proposed_media_path ?? item.error ?? item.note ?? 'n/a'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if preview.action === 'validate_nfo'}
				<ul class="rows mono">
					{#each preview.items as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.note ? `${item.proposed_media_path ?? 'nfo'} | ${item.note}` : item.error ?? 'n/a'}</strong>
						</li>
					{/each}
				</ul>
			{/if}
			{#if preview.action === 'combine_duplicates'}
				<ul class="rows mono">
					{#each preview.items as item}
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
							{#each preview.items as item}
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
											value={metadataOverrides[item.media_path]?.year ?? (item.metadata_year !== null ? String(item.metadata_year) : '')}
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
											value={metadataOverrides[item.media_path]?.confidence ?? (item.metadata_confidence !== null ? item.metadata_confidence.toFixed(2) : '')}
											oninput={(event) => updateMetadataDraft(item.media_path, 'confidence', (event.currentTarget as HTMLInputElement).value)}
										/>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
			{#if preview.items.some((item) => item.error)}
				<ul class="rows mono">
					{#each preview.items.filter((item) => item.error) as item}
						<li>
							<span>{item.media_path}</span>
							<strong>{item.error}</strong>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/if}

	{#if applyResult}
		<section class="card">
			<h2>Bulk Apply Result</h2>
			<p class="mono">
				total={applyResult.total_items} succeeded={applyResult.succeeded} failed={applyResult.failed}
			</p>
			{#if applyResult.action === 'rename'}
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
		</section>
	{/if}

	<section class="card table-card">
		{#if loading}
			<p class="mono">Loading library items...</p>
		{:else if error}
			<p class="error">{error}</p>
		{:else if items.length === 0}
			<p class="mono">No media items found for the current filters.</p>
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
		width: min(1280px, 92vw);
		margin: 0 auto 3rem;
		display: grid;
		gap: 1rem;
	}

	.library-hero {
		padding: 0.4rem 0;
	}

	.eyebrow {
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-size: 0.78rem;
		color: var(--muted);
		font-weight: 700;
	}

	.lead {
		max-width: 70ch;
		color: var(--muted);
	}

	.card {
		background: color-mix(in srgb, var(--card) 92%, transparent);
		border: 1px solid var(--ring);
		border-radius: 14px;
		padding: 1rem;
		backdrop-filter: blur(2px);
	}

	.control-row {
		display: flex;
		gap: 0.7rem;
		align-items: end;
		flex-wrap: wrap;
	}

	.control-row + .control-row {
		margin-top: 0.8rem;
	}

	label {
		display: grid;
		gap: 0.3rem;
		min-width: 240px;
	}

	.bulk-action-label {
		min-width: 220px;
	}

	label span {
		font-size: 0.85rem;
		color: var(--muted);
	}

	input,
	select,
	button {
		border-radius: 10px;
		border: 1px solid var(--ring);
		padding: 0.5rem 0.65rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
	}

	button {
		cursor: pointer;
		font-weight: 600;
	}

	button:disabled {
		opacity: 0.55;
		cursor: not-allowed;
	}

	.selection-pill {
		padding: 0.5rem 0.7rem;
		border-radius: 999px;
		border: 1px solid var(--ring);
	}

	.table-wrap {
		overflow-x: auto;
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.92rem;
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
		padding: 0.55rem;
		border-bottom: 1px solid var(--ring);
		text-align: left;
	}

	th {
		font-size: 0.76rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--muted);
	}

	.badge {
		display: inline-flex;
		padding: 0.2rem 0.5rem;
		border-radius: 999px;
		background: color-mix(in srgb, var(--danger) 12%, transparent);
		font-size: 0.8rem;
		font-weight: 700;
	}

	.badge.ok {
		background: color-mix(in srgb, var(--accent) 22%, transparent);
	}

	.pager {
		display: flex;
		gap: 0.6rem;
		justify-content: flex-end;
		margin-top: 0.9rem;
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
		gap: 0.45rem;
	}

	.rows li {
		display: flex;
		justify-content: space-between;
		gap: 0.7rem;
		border-bottom: 1px dashed var(--ring);
		padding-bottom: 0.3rem;
	}

	.queue-link {
		display: inline-flex;
		align-items: center;
		padding: 0.5rem 0.75rem;
		border-radius: 10px;
		border: 1px solid var(--ring);
		text-decoration: none;
		font-weight: 700;
	}

	.item-link {
		text-decoration: none;
		font-weight: 700;
		border-bottom: 1px dotted var(--ring);
	}

	@media (max-width: 900px) {
		.library-shell {
			width: min(100%, 96vw);
		}

		label {
			min-width: 180px;
		}
	}
</style>
