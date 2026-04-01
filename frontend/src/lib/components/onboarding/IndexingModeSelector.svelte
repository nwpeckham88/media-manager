<script lang="ts">
	import type { HashingMode } from '$lib/workflow/onboarding';

	let { value = $bindable<HashingMode>('hybrid') } = $props<{ value?: HashingMode }>();
</script>

<section class="panel" aria-label="Indexing Mode">
	<header>
		<p class="eyebrow">Step 2</p>
		<h2>Indexing Depth</h2>
		<p>Choose speed versus verification depth for your first index pass.</p>
	</header>

	<div class="modes">
		<label class:selected={value === 'hybrid'}>
			<input type="radio" name="mode" value="hybrid" bind:group={value} />
			<div>
				<strong>Hybrid (Recommended)</strong>
				<p>Enable ffprobe metadata, skip content hashing. Fast setup with strong metadata context.</p>
				<span class="mono">include_probe=true, include_hashes=false</span>
			</div>
		</label>

		<label class:selected={value === 'strict'}>
			<input type="radio" name="mode" value="strict" bind:group={value} />
			<div>
				<strong>Strict Hashing</strong>
				<p>Enable ffprobe and SHA256 hashing. More CPU and IO, best for duplicate certainty.</p>
				<span class="mono">include_probe=true, include_hashes=true</span>
			</div>
		</label>
	</div>
</section>

<style>
	.panel {
		display: grid;
		gap: var(--space-4);
	}

	header h2 {
		margin: var(--space-1) 0;
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
		font-size: var(--font-label);
		font-weight: 700;
		color: var(--muted);
	}

	.modes {
		display: grid;
		gap: var(--space-3);
	}

	label {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-3);
		align-items: start;
		border: 1px solid var(--ring);
		padding: var(--space-3);
		border-radius: 12px;
		cursor: pointer;
		background: color-mix(in srgb, var(--card) 92%, transparent);
	}

	label.selected {
		border-color: color-mix(in srgb, var(--accent) 55%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 26%, transparent);
	}

	input {
		margin-top: var(--space-1);
	}

	strong {
		font-size: 1rem;
	}

	p {
		margin: var(--space-1) 0;
		color: var(--muted);
	}

	span {
		font-size: var(--font-small);
		color: var(--muted);
	}
</style>
