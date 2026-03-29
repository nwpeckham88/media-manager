<script lang="ts">
	import { onMount } from 'svelte';

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

	let loading = $state(false);
	let error = $state('');
	let query = $state('');
	let onlyMissingProvider = $state(true);
	let maxConfidence = $state(0.95);
	let items = $state<IndexedMediaItem[]>([]);

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
		loading = false;
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
	<title>Media Manager | Metadata</title>
</svelte:head>

<main class="stage-shell">
	<section class="hero">
		<p class="eyebrow">Stage 2</p>
		<h1>Metadata</h1>
		<p class="lead">Review parser output and focus on items missing provider IDs or with lower-confidence metadata inference.</p>
	</section>

	<section class="card">
		<div class="actions">
			<input bind:value={query} placeholder="search title/path/provider" />
			<label class="toggle mono"><input type="checkbox" bind:checked={onlyMissingProvider} /> Only Missing Provider</label>
			<label class="mono">Max Confidence <input class="conf" type="number" min="0" max="1" step="0.01" bind:value={maxConfidence} /></label>
			<button type="button" onclick={refresh} disabled={loading}>Refresh</button>
			<a class="library-link" href="/library">Open Library Bulk Editor</a>
		</div>

		{#if error}
			<p class="error">{error}</p>
		{/if}
		{#if loading}
			<p class="mono">Loading metadata candidates...</p>
		{:else if items.length === 0}
			<p class="mono">No candidates matched current filters.</p>
		{:else}
			<div class="table-wrap">
				<table class="mono">
					<thead>
						<tr>
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
	}

	input,
	button,
	.library-link {
		border-radius: 10px;
		border: 1px solid var(--ring);
		padding: 0.45rem 0.6rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
	}

	button {
		font-weight: 700;
		cursor: pointer;
	}

	.library-link {
		text-decoration: none;
		font-weight: 700;
	}

	.table-wrap {
		overflow-x: auto;
		margin-top: 0.8rem;
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.9rem;
	}

	th,
	td {
		padding: 0.5rem;
		border-bottom: 1px solid var(--ring);
		text-align: left;
	}

	th {
		font-size: 0.75rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--muted);
	}

	.toggle {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
	}

	.conf {
		width: 5.6rem;
	}

	.error {
		color: var(--danger);
		font-weight: 700;
	}

</style>
