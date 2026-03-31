<script lang="ts">
	import { get } from 'svelte/store';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
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
	<PageHero
		eyebrow="Configuration Hub"
		title="High-Level Workflow Settings"
		lead="Configure indexing and merge policies in one place. These defaults are used by onboarding."
	>
		{#if notice}
			<p class="notice mono">{notice}</p>
		{/if}
	</PageHero>

	<SurfaceCard as="section">
		<h2>Onboarding Defaults</h2>
		<p class="mono muted summary-line">These values prefill the guided setup flow on first boot.</p>
		<div class="field-grid">
			<article>
				<p class="label mono">Default Indexing Mode</p>
				<label><input type="radio" name="hashing" value="hybrid" bind:group={hashingMode} /> Hybrid (recommended)</label>
				<label><input type="radio" name="hashing" value="strict" bind:group={hashingMode} /> Strict hashing</label>
			</article>
			<article>
				<p class="label mono">Rename Pattern</p>
				<label><input type="radio" checked disabled /> Movie Name - Subtitle (Year)</label>
				<p class="muted">Canonical naming is mandatory for semantic-merge normalization.</p>
			</article>
		</div>
	</SurfaceCard>

	<SurfaceCard as="section">
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
	</SurfaceCard>

	<div class="actions">
		<button type="button" onclick={saveSettings}>Save Settings</button>
	</div>
</main>

<style>
	.settings-shell {
		width: min(var(--content-max), 94vw);
		margin: var(--space-4) auto calc(var(--space-6) * 2);
		display: grid;
		gap: var(--space-4);
	}

	.summary-line,
	.muted {
		margin: 0;
		color: var(--muted);
		font-size: var(--font-small);
	}

	.notice {
		margin: 0;
		color: var(--accent);
		font-size: var(--font-small);
	}

	.field-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--space-3);
	}

	.field-grid.one-col {
		grid-template-columns: 1fr;
	}

	article {
		display: grid;
		gap: var(--space-2);
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: var(--space-3);
		background: color-mix(in srgb, var(--card) 95%, transparent);
	}

	.label {
		margin: 0;
		font-size: var(--font-label);
		text-transform: uppercase;
		letter-spacing: 0.09em;
		color: var(--muted);
	}

	label {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		font-weight: 600;
		font-size: var(--font-small);
	}

	.actions {
		display: flex;
		justify-content: flex-end;
	}

	button {
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		padding: 0.52rem 0.82rem;
		font: inherit;
		font-weight: 700;
		font-size: var(--font-small);
		background: color-mix(in srgb, var(--card) 95%, transparent);
		cursor: pointer;
	}

	button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	@media (max-width: 820px) {
		.field-grid {
			grid-template-columns: 1fr;
		}

		button {
			width: 100%;
		}

		.actions {
			justify-content: stretch;
		}
	}
</style>
