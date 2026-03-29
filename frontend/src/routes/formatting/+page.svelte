<script lang="ts">
	import { onMount } from 'svelte';

	type FormattingCandidateItem = {
		media_path: string;
		proposed_media_path: string;
		note: string;
	};

	type FormattingCandidatesResponse = {
		total_items: number;
		items: FormattingCandidateItem[];
	};

	type BulkDryRunResponse = {
		batch_hash: string;
		total_items: number;
		plan_ready: boolean;
		summary: {
			creates: number;
			updates: number;
			noops: number;
			invalid: number;
		};
	};

	type BulkApplyResponse = {
		total_items: number;
		succeeded: number;
		failed: number;
	};

	let loading = $state(false);
	let error = $state('');
	let items = $state<FormattingCandidateItem[]>([]);
	let selectedPaths = $state<string[]>([]);
	let busy = $state(false);
	let preview = $state<BulkDryRunResponse | null>(null);
	let applyResult = $state<BulkApplyResponse | null>(null);

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
				item_uid: file.replace(/\.[^.]+$/, '')
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
		applyResult = null;
		const response = await apiFetch('/api/bulk/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'rename',
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
			error = 'Run preview first.';
			return;
		}
		if (!preview.plan_ready) {
			error = 'Preview has invalid items. Resolve before apply.';
			return;
		}

		busy = true;
		error = '';
		const response = await apiFetch('/api/bulk/apply', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'rename',
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
		busy = false;
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
	<title>Media Manager | Formatting</title>
</svelte:head>

<main class="stage-shell">
	<section class="hero">
		<p class="eyebrow">Stage 3</p>
		<h1>Formatting</h1>
		<p class="lead">Review deterministic rename candidates generated from the indexed library snapshot before applying formatting operations.</p>
	</section>

	<section class="card">
		<div class="actions">
			<button type="button" onclick={refresh} disabled={loading}>Refresh</button>
			<button type="button" onclick={selectAll} disabled={loading || items.length === 0}>Select All</button>
			<button type="button" onclick={clearSelection} disabled={loading || selectedPaths.length === 0}>Clear</button>
			<button type="button" onclick={runPreview} disabled={busy || selectedPaths.length === 0}>Preview Rename</button>
			<button type="button" onclick={applyPreview} disabled={busy || !preview || !preview.plan_ready}>Apply Rename</button>
			<a class="library-link" href="/library">Open Library Rename/NFO Actions</a>
		</div>

		<p class="mono">selected={selectedPaths.length}</p>
		{#if preview}
			<p class="mono">preview batch={preview.batch_hash} total={preview.total_items} invalid={preview.summary.invalid}</p>
		{/if}
		{#if applyResult}
			<p class="mono">applied total={applyResult.total_items} ok={applyResult.succeeded} fail={applyResult.failed}</p>
		{/if}

		{#if error}
			<p class="error">{error}</p>
		{/if}

		{#if loading}
			<p class="mono">Loading formatting candidates...</p>
		{:else if items.length === 0}
			<p class="mono">No rename candidates found in current indexed window.</p>
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

	button,
	.library-link {
		border-radius: 10px;
		border: 1px solid var(--ring);
		padding: 0.45rem 0.6rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
		font-weight: 700;
		text-decoration: none;
	}

	button {
		cursor: pointer;
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

	.error {
		color: var(--danger);
		font-weight: 700;
	}

</style>
