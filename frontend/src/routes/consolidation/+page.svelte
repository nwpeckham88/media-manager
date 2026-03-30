<script lang="ts">
	import { onMount } from 'svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import { markStageComplete, markStageIncomplete } from '$lib/workflow/progress';

	type IndexStatsResponse = {
		total_indexed: number;
		hashed: number;
		probed: number;
		last_indexed_at_ms: number | null;
	};

	type ExactDuplicateItem = {
		media_path: string;
		file_size: number;
		parsed_title: string | null;
		parsed_year: number | null;
		parsed_provider_id: string | null;
	};

	type ExactDuplicateGroup = {
		content_hash: string;
		count: number;
		items: ExactDuplicateItem[];
	};

	type ExactDuplicatesResponse = {
		total_groups: number;
		groups: ExactDuplicateGroup[];
	};

	type SemanticDuplicateItem = {
		media_path: string;
		file_size: number;
		content_hash: string | null;
		video_codec: string | null;
		audio_codec: string | null;
		width: number | null;
		height: number | null;
	};

	type SemanticDuplicateGroup = {
		parsed_title: string;
		parsed_year: number | null;
		parsed_provider_id: string | null;
		item_count: number;
		variant_count: number;
		items: SemanticDuplicateItem[];
	};

	type SemanticDuplicatesResponse = {
		total_groups: number;
		groups: SemanticDuplicateGroup[];
	};

	type ConsolidationQuarantineResponse = {
		total_items: number;
		succeeded: number;
		failed: number;
		items: {
			media_path: string;
			success: boolean;
			operation_id: string | null;
		}[];
	};

	type BulkDryRunResponse = {
		batch_hash: string;
		total_items: number;
		plan_ready: boolean;
		summary: {
			invalid: number;
		};
	};

	type BulkApplyResponse = {
		total_items: number;
		succeeded: number;
		failed: number;
		items: {
			media_path: string;
			success: boolean;
			operation_id: string | null;
			error: string | null;
		}[];
	};

	type BulkRollbackResponse = {
		total_items: number;
		succeeded: number;
		failed: number;
	};

	let loading = $state(false);
	let indexing = $state(false);
	let error = $state('');
	let notice = $state('');
	let stats = $state<IndexStatsResponse | null>(null);
	let exactGroups = $state<ExactDuplicateGroup[]>([]);
	let semanticGroups = $state<SemanticDuplicateGroup[]>([]);
	let mergingKey = $state<string | null>(null);
	let quarantiningKey = $state<string | null>(null);
	let rollbacking = $state(false);
	let rollbackOperationIds = $state<string[]>([]);
	let rollbackResult = $state<BulkRollbackResponse | null>(null);

	onMount(async () => {
		await refresh();
	});

	async function refresh() {
		loading = true;
		error = '';
		await Promise.all([loadStats(), loadExactDuplicateGroups(), loadSemanticDuplicateGroups()]);
		loading = false;
	}

	async function loadStats() {
		const response = await apiFetch('/api/index/stats');
		if (!response.ok) {
			error = `Unable to load index stats (${response.status})`;
			return;
		}

		stats = (await response.json()) as IndexStatsResponse;
	}

	async function loadExactDuplicateGroups() {
		const response = await apiFetch('/api/consolidation/exact-duplicates?limit=30&min_group_size=2');
		if (!response.ok) {
			error = `Unable to load duplicate groups (${response.status})`;
			return;
		}

		const payload = (await response.json()) as ExactDuplicatesResponse;
		exactGroups = payload.groups;
	}

	async function loadSemanticDuplicateGroups() {
		const response = await apiFetch('/api/consolidation/semantic-duplicates?limit=30&min_group_size=2');
		if (!response.ok) {
			error = `Unable to load semantic groups (${response.status})`;
			return;
		}

		const payload = (await response.json()) as SemanticDuplicatesResponse;
		semanticGroups = payload.groups;
	}

	async function startIndexing() {
		indexing = true;
		error = '';
		notice = '';

		const response = await apiFetch('/api/index/start', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ include_hashes: true, include_probe: true })
		});
		if (!response.ok) {
			error = await response.text();
			indexing = false;
			return;
		}

		const payload = (await response.json()) as { job_id: number };
		notice = `Index started: job #${payload.job_id}. Next: monitor Queue.`;
		indexing = false;
		await refresh();
	}

	function canonicalUidForGroup(group: SemanticDuplicateGroup): string {
		const base = group.parsed_title
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '');
		return `${base}${group.parsed_year ? `-${group.parsed_year}` : ''}`;
	}

	function groupKey(group: SemanticDuplicateGroup): string {
		return `${group.parsed_title}|${group.parsed_year ?? 'none'}|${group.parsed_provider_id ?? 'none'}`;
	}

	async function mergeSemanticGroup(group: SemanticDuplicateGroup) {
		if (group.items.length < 2) {
			return;
		}

		const key = groupKey(group);
		mergingKey = key;
		error = '';
		notice = '';

		const uid = canonicalUidForGroup(group);
		const itemsPayload = group.items.map((item) => ({
			media_path: item.media_path,
			item_uid: uid
		}));

		const previewResponse = await apiFetch('/api/bulk/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'combine_duplicates',
				items: itemsPayload
			})
		});
		if (!previewResponse.ok) {
			error = await previewResponse.text();
			mergingKey = null;
			return;
		}

		const preview = (await previewResponse.json()) as BulkDryRunResponse;
		if (!preview.plan_ready) {
			error = 'Merge preview includes invalid items. Resolve them before apply.';
			mergingKey = null;
			return;
		}

		const applyResponse = await apiFetch('/api/bulk/apply', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'combine_duplicates',
				approved_batch_hash: preview.batch_hash,
				items: itemsPayload
			})
		});
		if (!applyResponse.ok) {
			error = await applyResponse.text();
			mergingKey = null;
			return;
		}

		const result = (await applyResponse.json()) as BulkApplyResponse;
		const operationIds = result.items
			.filter((item) => item.success && !!item.operation_id)
			.map((item) => item.operation_id as string);
		if (operationIds.length > 0) {
			rollbackOperationIds = [...rollbackOperationIds, ...operationIds];
		}
		let renameNote = '';
		if (result.succeeded > 0) {
			const renameOutcome = await normalizeSemanticGroupNames(group, uid);
			if (renameOutcome) {
				renameNote = ` | canonical rename ok=${renameOutcome.succeeded} fail=${renameOutcome.failed}`;
			}
		}

		if (result.failed === 0) {
			markStageComplete('consolidation');
		}
		notice = `Merge complete: uid=${uid} (ok=${result.succeeded}, fail=${result.failed})${renameNote}.`;
		mergingKey = null;
		await refresh();
	}

	async function normalizeSemanticGroupNames(group: SemanticDuplicateGroup, uid: string): Promise<BulkApplyResponse | null> {
		const preferredTitle = group.parsed_title?.trim();
		if (!preferredTitle) {
			return null;
		}

		const itemsPayload = group.items.map((item) => ({
			media_path: item.media_path,
			item_uid: uid,
			metadata_override: {
				title: preferredTitle,
				year: group.parsed_year ?? undefined
			}
		}));

		const previewResponse = await apiFetch('/api/bulk/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'rename',
				items: itemsPayload
			})
		});
		if (!previewResponse.ok) {
			error = `Rename preview failed: ${await previewResponse.text()}`;
			return null;
		}

		const preview = (await previewResponse.json()) as BulkDryRunResponse;
		if (!preview.plan_ready) {
			error = 'Rename preview contains invalid items. Resolve naming conflicts before retry.';
			return null;
		}

		const applyResponse = await apiFetch('/api/bulk/apply', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'rename',
				approved_batch_hash: preview.batch_hash,
				items: itemsPayload
			})
		});
		if (!applyResponse.ok) {
			error = `Rename apply failed: ${await applyResponse.text()}`;
			return null;
		}

		const applyResult = (await applyResponse.json()) as BulkApplyResponse;
		const operationIds = applyResult.items
			.filter((item) => item.success && !!item.operation_id)
			.map((item) => item.operation_id as string);
		if (operationIds.length > 0) {
			rollbackOperationIds = [...rollbackOperationIds, ...operationIds];
		}

		return applyResult;
	}

	function formatBytes(bytes: number): string {
		if (bytes < 1024) {
			return `${bytes} B`;
		}
		const kib = bytes / 1024;
		if (kib < 1024) {
			return `${kib.toFixed(1)} KiB`;
		}
		const mib = kib / 1024;
		if (mib < 1024) {
			return `${mib.toFixed(1)} MiB`;
		}
		return `${(mib / 1024).toFixed(2)} GiB`;
	}

	function exactGroupKey(group: ExactDuplicateGroup): string {
		return group.content_hash;
	}

	async function quarantineExactGroup(group: ExactDuplicateGroup) {
		if (group.items.length < 2) {
			return;
		}

		const key = exactGroupKey(group);
		quarantiningKey = key;
		error = '';
		notice = '';

		const sortedBySize = [...group.items].sort((a, b) => b.file_size - a.file_size);
		const keep = sortedBySize[0];
		const payload = {
			keep_media_path: keep.media_path,
			media_paths: group.items.map((item) => item.media_path)
		};

		const response = await apiFetch('/api/consolidation/quarantine', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(payload)
		});
		if (!response.ok) {
			error = await response.text();
			quarantiningKey = null;
			return;
		}

		const result = (await response.json()) as ConsolidationQuarantineResponse;
		const operationIds = result.items
			.filter((item) => item.success && !!item.operation_id)
			.map((item) => item.operation_id as string);
		if (operationIds.length > 0) {
			rollbackOperationIds = [...rollbackOperationIds, ...operationIds];
		}
		if (result.failed === 0) {
			markStageComplete('consolidation');
		}
		notice = `Quarantine complete: hash ${group.content_hash.slice(0, 12)}... (ok=${result.succeeded}, fail=${result.failed}).`;
		quarantiningKey = null;
		await refresh();
	}

	async function rollbackRecentOps() {
		if (rollbackOperationIds.length === 0) {
			error = 'No rollback operations are available in this session.';
			return;
		}

		rollbacking = true;
		error = '';
		notice = '';
		const response = await apiFetch('/api/bulk/rollback', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ operation_ids: rollbackOperationIds })
		});
		if (!response.ok) {
			error = await response.text();
			rollbacking = false;
			return;
		}

		rollbackResult = (await response.json()) as BulkRollbackResponse;
		notice = `Rollback complete: ok=${rollbackResult.succeeded}, fail=${rollbackResult.failed}.`;
		if (rollbackResult.failed === 0) {
			rollbackOperationIds = [];
			markStageIncomplete('consolidation');
		}
		rollbacking = false;
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
	<title>Media Manager | Consolidation</title>
</svelte:head>

<main class="stage-shell">
	<section class="hero">
		<p class="eyebrow">Stage 1</p>
		<h1>Consolidation</h1>
		<p class="lead">Run library-wide indexing (hash + ffprobe), then review exact duplicate files before metadata and formatting passes.</p>
	</section>

	<section class="card">
		<div class="actions">
			<button type="button" onclick={startIndexing} disabled={indexing || loading}>Start Full Index</button>
			<button type="button" onclick={refresh} disabled={loading}>Refresh</button>
			<button type="button" onclick={rollbackRecentOps} disabled={rollbacking || rollbackOperationIds.length === 0}>
				{rollbacking ? 'Rolling Back...' : 'Rollback Session Ops'}
			</button>
			<a class="queue-link" href="/queue">Queue</a>
		</div>
		<OperationResultBanner notice={notice} error={error} nextHref="/metadata" nextLabel="Next: Metadata" />

		{#if rollbackResult}
			<p class="mono">rollback total={rollbackResult.total_items} ok={rollbackResult.succeeded} fail={rollbackResult.failed}</p>
		{/if}

		{#if stats}
			<ul class="rows mono">
				<li><span>Indexed Files</span><strong>{stats.total_indexed}</strong></li>
				<li><span>Hashed Files</span><strong>{stats.hashed}</strong></li>
				<li><span>FFprobe Enriched</span><strong>{stats.probed}</strong></li>
				<li><span>Last Index</span><strong>{stats.last_indexed_at_ms ? new Date(stats.last_indexed_at_ms).toLocaleString() : 'never'}</strong></li>
			</ul>
		{/if}
	</section>

	<section class="card">
		<h2>Exact Duplicate Groups</h2>
		{#if loading}
			<p class="mono">Loading groups...</p>
		{:else if exactGroups.length === 0}
			<p class="mono">No exact duplicate groups detected yet.</p>
		{:else}
			<div class="group-grid">
				{#each exactGroups as group}
					{@const key = exactGroupKey(group)}
					<article class="group-card">
						<p class="mono">hash={group.content_hash.slice(0, 16)}... count={group.count}</p>
						<div class="actions">
							<button type="button" onclick={() => quarantineExactGroup(group)} disabled={quarantiningKey !== null && quarantiningKey !== key}>
								{quarantiningKey === key ? 'Quarantining...' : 'Keep Largest, Quarantine Rest'}
							</button>
						</div>
						<ul class="rows mono">
							{#each group.items as item}
								<li>
									<span>{item.media_path}</span>
									<strong>{formatBytes(item.file_size)}</strong>
								</li>
							{/each}
						</ul>
					</article>
				{/each}
			</div>
		{/if}
	</section>

	<section class="card">
		<h2>Semantic Duplicate Groups</h2>
		<p class="mono">Same parsed title/year/provider with multiple file variants.</p>
		<p class="mono merge-policy">
			Merge policy: canonical naming is mandatory. All matched items are normalized to
			<code>Movie Title (Year)</code> with invalid characters replaced by <code>-</code>, and linked stem-matching
			metadata assets are renamed with the media file.
		</p>
		{#if loading}
			<p class="mono">Loading semantic groups...</p>
		{:else if semanticGroups.length === 0}
			<p class="mono">No semantic duplicate groups detected yet.</p>
		{:else}
			<div class="group-grid">
				{#each semanticGroups as group}
					{@const key = groupKey(group)}
					<article class="group-card">
						<p class="mono">
							title={group.parsed_title} year={group.parsed_year ?? 'unknown'} provider={group.parsed_provider_id ?? 'none'}
						</p>
						<p class="mono">items={group.item_count} variants={group.variant_count}</p>
						<div class="actions">
							<button type="button" onclick={() => mergeSemanticGroup(group)} disabled={mergingKey !== null && mergingKey !== key}>
								{mergingKey === key ? 'Merging...' : `Merge IDs -> ${canonicalUidForGroup(group)}`}
							</button>
						</div>
						<ul class="rows mono">
							{#each group.items as item}
								<li>
									<span>{item.media_path}</span>
									<strong>
										{formatBytes(item.file_size)}
										{item.width && item.height ? ` | ${item.width}x${item.height}` : ''}
										{item.video_codec ? ` | v:${item.video_codec}` : ''}
										{item.audio_codec ? ` | a:${item.audio_codec}` : ''}
									</strong>
								</li>
							{/each}
						</ul>
					</article>
				{/each}
			</div>
		{/if}
	</section>
</main>

<style>
	.stage-shell {
		width: min(1100px, 92vw);
		margin: 0 auto 3rem;
		display: grid;
		gap: 1rem;
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
	}

	.lead {
		max-width: 72ch;
		color: var(--muted);
	}

	.card {
		background: color-mix(in srgb, var(--card) 92%, transparent);
		border: 1px solid var(--ring);
		border-radius: 14px;
		padding: 1rem;
		backdrop-filter: blur(2px);
	}

	.actions {
		display: flex;
		gap: 0.7rem;
		align-items: center;
		flex-wrap: wrap;
		margin-bottom: 0.8rem;
	}

	button,
	.queue-link {
		border-radius: 10px;
		border: 1px solid var(--ring);
		padding: 0.5rem 0.65rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
		font-weight: 700;
		text-decoration: none;
	}

	button {
		cursor: pointer;
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

	.group-grid {
		display: grid;
		gap: 0.8rem;
	}

	.group-card {
		border: 1px solid var(--ring);
		border-radius: 10px;
		padding: 0.7rem;
		background: color-mix(in srgb, var(--card) 87%, transparent);
	}

	.merge-policy {
		margin: 0.55rem 0 0.9rem;
		font-size: 0.9rem;
		color: var(--muted);
	}

</style>
