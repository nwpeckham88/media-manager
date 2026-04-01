<script lang="ts">
	import type { NamingFormat } from '$lib/workflow/onboarding';

	let { value = $bindable<NamingFormat>('movie_title_subtitle_year') } = $props<{ value?: NamingFormat }>();
</script>

<section class="panel" aria-label="Naming Format">
	<header>
		<p class="eyebrow">Step 3</p>
		<h2>Naming Format</h2>
		<p>Define the final filename format your library should converge toward.</p>
	</header>

	<label class="choice">
		<input type="radio" name="naming-format" value="movie_title_subtitle_year" bind:group={value} />
		<div>
			<strong>Movie Title - Subtitle (Year)</strong>
			<p><span class="mono">Blade Runner - Final Cut (1982).mkv</span></p>
			<p class="note">Recommended for preserving subtitle/version detail in the filename.</p>
		</div>
	</label>

	<label class="choice">
		<input type="radio" name="naming-format" value="movie_title_year" bind:group={value} />
		<div>
			<strong>Movie Title (Year)</strong>
			<p><span class="mono">Blade Runner (1982).mkv</span></p>
			<p class="note">More compact naming that trims subtitle suffixes from movie filenames.</p>
		</div>
	</label>

	<p class="muted">TV episode naming remains Jellyfin-safe: <span class="mono">Show - S01E01 - Episode Title.ext</span>.</p>
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

	.note,
	.muted {
		font-size: var(--font-small);
		color: var(--muted);
		margin: 0;
	}
</style>
