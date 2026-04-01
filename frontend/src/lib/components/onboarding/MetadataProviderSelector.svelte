<script lang="ts">
	import type { MetadataProvider } from '$lib/workflow/onboarding';

	let { value = $bindable<MetadataProvider>('tmdb') } = $props<{ value?: MetadataProvider }>();
</script>

<section class="panel" aria-label="Metadata Provider">
	<header>
		<p class="eyebrow">Step 3</p>
		<h2>Metadata Provider</h2>
		<p>Pick the provider that should be treated as your library source of truth.</p>
	</header>

	<label class="choice">
		<input type="radio" name="metadata-provider" value="tmdb" bind:group={value} />
		<div>
			<strong>TMDB</strong>
			<p>Best default for movie-focused libraries and broad community metadata.</p>
		</div>
	</label>

	<label class="choice">
		<input type="radio" name="metadata-provider" value="imdb" bind:group={value} />
		<div>
			<strong>IMDb</strong>
			<p>Prefer IMDb IDs (<span class="mono">tt1234567</span>) for compatibility with IMDb-centric tooling.</p>
		</div>
	</label>

	<label class="choice">
		<input type="radio" name="metadata-provider" value="tvdb" bind:group={value} />
		<div>
			<strong>TVDB</strong>
			<p>Useful for TV-heavy libraries where TVDB IDs are your canonical identifier.</p>
		</div>
	</label>
</section>

<style>
	.panel {
		display: grid;
		gap: var(--space-3);
	}

	header h2 {
		margin: var(--space-1) 0;
		font-size: 1.15rem;
	}

	header p {
		margin: 0;
		color: var(--muted);
	}

	.eyebrow {
		margin: 0;
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-size: var(--font-label);
		font-weight: 700;
		color: var(--muted);
	}

	.choice {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-2);
		align-items: start;
		border: 1px solid var(--ring);
		border-radius: 10px;
		padding: var(--space-2);
		background: color-mix(in srgb, var(--card) 92%, transparent);
	}

	.choice p {
		margin: var(--space-1) 0 0;
		color: var(--muted);
		font-size: var(--font-small);
	}
</style>
