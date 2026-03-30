<script lang="ts">
	import { get } from 'svelte/store';
	import { appSettings, updateAppSettings, type DashboardRefreshPolicy } from '$lib/workflow/settings';
	import type { HashingMode } from '$lib/workflow/onboarding';

	const current = get(appSettings);
	let hashingMode = $state<HashingMode>(current.defaultHashingMode);
	let refreshPolicy = $state<DashboardRefreshPolicy>(current.dashboardRefreshPolicy);
	let notice = $state('');

	function saveSettings() {
		updateAppSettings({
			defaultHashingMode: hashingMode,
			dashboardRefreshPolicy: refreshPolicy,
			renamePreset: 'movie_year'
		});
		notice = 'Settings saved. Onboarding defaults and runtime behavior updated.';
	}
</script>

<svelte:head>
	<title>Media Manager | Settings</title>
</svelte:head>

<main class="settings-shell">
	<section class="hero card">
		<p class="eyebrow">Configuration Hub</p>
		<h1>High-Level Workflow Settings</h1>
		<p>Configure indexing and merge policies in one place. These defaults are used by onboarding.</p>
		{#if notice}
			<p class="notice mono">{notice}</p>
		{/if}
	</section>

	<section class="card">
		<h2>Onboarding Defaults</h2>
		<p class="mono muted">These values prefill the guided setup flow on first boot.</p>
		<div class="field-grid">
			<article>
				<p class="label mono">Default Indexing Mode</p>
				<label><input type="radio" name="hashing" value="hybrid" bind:group={hashingMode} /> Hybrid (recommended)</label>
				<label><input type="radio" name="hashing" value="strict" bind:group={hashingMode} /> Strict hashing</label>
			</article>
			<article>
				<p class="label mono">Rename Pattern</p>
				<label><input type="radio" checked disabled /> Movie Title (Year)</label>
				<p class="muted">Canonical naming is mandatory for semantic-merge normalization.</p>
			</article>
		</div>
	</section>

	<section class="card">
		<h2>Runtime Behavior</h2>
		<div class="field-grid one-col">
			<article>
				<p class="label mono">Dashboard Auto Refresh</p>
				<label><input type="radio" name="refresh" value="running-jobs-only" bind:group={refreshPolicy} /> Only when jobs are running (recommended)</label>
				<label><input type="radio" name="refresh" value="always" bind:group={refreshPolicy} /> Always refresh on interval</label>
				<label><input type="radio" name="refresh" value="manual" bind:group={refreshPolicy} /> Manual only (Refresh button/pages)</label>
			</article>
			<article>
				<p class="label mono">Duplicate Handling Policy</p>
				<p class="muted">Exact duplicates: user must choose which file to keep in modal.</p>
				<p class="muted">Semantic duplicates: canonical naming normalization is mandatory after merge.</p>
			</article>
		</div>
	</section>

	<div class="actions">
		<button type="button" onclick={saveSettings}>Save Settings</button>
	</div>
</main>

<style>
	.settings-shell {
		width: min(980px, 94vw);
		margin: 1rem auto 3rem;
		display: grid;
		gap: 0.9rem;
	}

	.card {
		border: 1px solid var(--ring);
		border-radius: 14px;
		padding: 1rem;
		background: color-mix(in srgb, var(--card) 92%, transparent);
	}

	.hero h1 {
		margin: 0.25rem 0;
	}

	.hero p,
	.muted {
		margin: 0;
		color: var(--muted);
	}

	.eyebrow {
		margin: 0;
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-size: 0.75rem;
		font-weight: 700;
		color: var(--muted);
	}

	.notice {
		margin-top: 0.45rem;
		color: var(--accent);
	}

	.field-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.8rem;
	}

	.field-grid.one-col {
		grid-template-columns: 1fr;
	}

	article {
		display: grid;
		gap: 0.45rem;
		border: 1px solid var(--ring);
		border-radius: 10px;
		padding: 0.72rem;
		background: color-mix(in srgb, var(--card) 95%, transparent);
	}

	.label {
		margin: 0;
		font-size: 0.76rem;
		text-transform: uppercase;
		letter-spacing: 0.09em;
		color: var(--muted);
	}

	label {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-weight: 600;
	}

	.actions {
		display: flex;
		justify-content: flex-end;
	}

	button {
		border-radius: 10px;
		border: 1px solid var(--ring);
		padding: 0.52rem 0.82rem;
		font: inherit;
		font-weight: 700;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		cursor: pointer;
	}

	@media (max-width: 820px) {
		.field-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
