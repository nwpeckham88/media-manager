<script lang="ts">
	import { onMount } from 'svelte';

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

	let loading = $state(false);
	let indexing = $state(false);
	let error = $state('');
	let notice = $state('');
	let stats = $state<IndexStatsResponse | null>(null);
	let exactGroups = $state<ExactDuplicateGroup[]>([]);
	let semanticGroups = $state<SemanticDuplicateGroup[]>([]);

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
		notice = `Index job #${payload.job_id} started. Track progress in Queue.`;
		indexing = false;
		await refresh();
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
			<a class="queue-link" href="/queue">Queue</a>
		</div>

		{#if notice}
			<p class="notice mono">{notice}</p>
		{/if}
		{#if error}
			<p class="error">{error}</p>
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
					<article class="group-card">
						<p class="mono">hash={group.content_hash.slice(0, 16)}... count={group.count}</p>
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
		{#if loading}
			<p class="mono">Loading semantic groups...</p>
		{:else if semanticGroups.length === 0}
			<p class="mono">No semantic duplicate groups detected yet.</p>
		{:else}
			<div class="group-grid">
				{#each semanticGroups as group}
					<article class="group-card">
						<p class="mono">
							title={group.parsed_title} year={group.parsed_year ?? 'unknown'} provider={group.parsed_provider_id ?? 'none'}
						</p>
						<p class="mono">items={group.item_count} variants={group.variant_count}</p>
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

	.error {
		color: var(--danger);
		font-weight: 700;
	}

	.notice {
		color: var(--accent);
		font-weight: 700;
	}
</style>
