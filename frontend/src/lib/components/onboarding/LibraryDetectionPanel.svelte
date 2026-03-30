<script lang="ts">
	import type { ApiState, AppConfigResponse, ScanSummary } from './types';

	let {
		configState,
		scanState
	} = $props<{
		configState: ApiState<AppConfigResponse>;
		scanState: ApiState<ScanSummary>;
	}>();

	const healthyRoots = $derived.by(() => {
		if (!scanState.ok || !scanState.data) {
			return 0;
		}

		return scanState.data.roots.filter((root: { exists: boolean; error: string | null }) => root.exists && !root.error)
			.length;
	});
</script>

<section class="panel">
	<header>
		<p class="eyebrow">Step 1</p>
		<h2>Library Detection</h2>
		<p>Confirm that Media Manager sees your configured Jellyfin roots before any indexing starts.</p>
	</header>

	{#if configState.ok && configState.data}
		<div class="meta-row">
			<div>
				<strong>{configState.data.library_roots.length}</strong>
				<span>configured roots</span>
			</div>
			<div>
				<strong>{healthyRoots}</strong>
				<span>roots detected cleanly</span>
			</div>
		</div>
	{:else}
		<p class="error">{configState.error ?? 'Unable to load app config.'}</p>
	{/if}

	{#if scanState.ok && scanState.data}
		<ul class="root-list">
			{#each scanState.data.roots as root}
				<li class:root-error={!root.exists || !!root.error}>
					<div>
						<p class="root-path mono">{root.root}</p>
						<p class="root-note">
							{#if root.exists && !root.error}
								Detected successfully
							{:else if root.error}
								{root.error}
							{:else}
								Root path does not exist
							{/if}
						</p>
					</div>
					<strong>{root.media_files.toLocaleString()} files</strong>
				</li>
			{/each}
		</ul>
		<p class="total mono">Total media files detected: {scanState.data.total_media_files.toLocaleString()}</p>
	{:else}
		<p class="error">{scanState.error ?? 'Scan summary unavailable.'}</p>
	{/if}
</section>

<style>
	.panel {
		display: grid;
		gap: 0.9rem;
	}

	header h2 {
		margin: 0.2rem 0;
		font-size: 1.3rem;
	}

	header p {
		margin: 0;
		color: var(--muted);
	}

	.eyebrow {
		margin: 0;
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-size: 0.72rem;
		font-weight: 700;
		color: var(--muted);
	}

	.meta-row {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.55rem;
	}

	.meta-row div {
		border: 1px solid var(--ring);
		border-radius: 12px;
		padding: 0.65rem;
		background: color-mix(in srgb, var(--card) 90%, transparent);
		display: grid;
		gap: 0.1rem;
	}

	.meta-row strong {
		font-size: 1.4rem;
	}

	.meta-row span {
		font-size: 0.8rem;
		color: var(--muted);
	}

	.root-list {
		list-style: none;
		padding: 0;
		margin: 0;
		display: grid;
		gap: 0.45rem;
	}

	.root-list li {
		display: flex;
		justify-content: space-between;
		gap: 0.5rem;
		padding: 0.65rem;
		border: 1px solid var(--ring);
		border-radius: 12px;
		background: color-mix(in srgb, var(--card) 95%, transparent);
	}

	.root-list li.root-error {
		border-color: color-mix(in srgb, var(--danger) 46%, var(--ring));
	}

	.root-path {
		margin: 0;
		font-size: 0.76rem;
		word-break: break-all;
	}

	.root-note {
		margin: 0.2rem 0 0;
		font-size: 0.83rem;
		color: var(--muted);
	}

	.total {
		margin: 0;
		font-size: 0.8rem;
		color: var(--muted);
	}

	.error {
		margin: 0;
		color: var(--danger);
		font-weight: 700;
	}

	@media (max-width: 780px) {
		.meta-row {
			grid-template-columns: 1fr;
		}

		.root-list li {
			flex-direction: column;
		}
	}
</style>
