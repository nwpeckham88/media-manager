<script lang="ts">
	import type { RenamePreset } from '$lib/workflow/onboarding';

	let { value = $bindable<RenamePreset>('movie_year') } = $props<{ value?: RenamePreset }>();
</script>

<section class="panel" aria-label="Rename Preset">
	<header>
		<p class="eyebrow">Step 4</p>
		<h2>Naming Strategy</h2>
		<p>For this release, setup exposes one Jellyfin-safe naming standard.</p>
	</header>

	<label class="preset selected">
		<input type="radio" name="rename-preset" value="movie_year" bind:group={value} />
		<div>
			<strong>Movie Name (Year)</strong>
			<p>Example: <span class="mono">The Matrix (1999).mkv</span></p>
			<p class="note">Keeps directories stable and portable without app-specific metadata dependencies.</p>
		</div>
	</label>
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

	.preset {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 0.7rem;
		align-items: start;
		border: 1px solid color-mix(in srgb, var(--accent) 55%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 26%, transparent);
		padding: 0.75rem;
		border-radius: 12px;
		background: color-mix(in srgb, var(--card) 92%, transparent);
	}

	input {
		margin-top: 0.25rem;
	}

	strong {
		font-size: 1rem;
	}

	p {
		margin: 0.24rem 0;
		color: var(--muted);
	}

	.note {
		font-size: 0.83rem;
	}
</style>
