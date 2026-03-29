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

	let loading = $state(false);
	let error = $state('');
	let items = $state<FormattingCandidateItem[]>([]);

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
			<a class="library-link" href="/library">Open Library Rename/NFO Actions</a>
		</div>

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
							<th>Current Path</th>
							<th>Proposed Path</th>
							<th>Note</th>
						</tr>
					</thead>
					<tbody>
						{#each items as item}
							<tr>
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
