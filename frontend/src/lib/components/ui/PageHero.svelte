<script lang="ts">
	import type { Snippet } from 'svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';

	let {
		eyebrow,
		title,
		lead,
		stamp = '',
		children
	} = $props<{
		eyebrow: string;
		title: string;
		lead: string;
		stamp?: string;
		children?: Snippet;
	}>();
</script>

<section class="page-hero">
	<SurfaceCard tone="hero" as="div">
		<div>
			<p class="eyebrow">{eyebrow}</p>
			<h1>{title}</h1>
			<p class="lead">{lead}</p>
		</div>
		{#if stamp}
			<p class="mono stamp">{stamp}</p>
		{/if}
		<div class="actions">
			{@render children?.()}
		</div>
	</SurfaceCard>
</section>

<style>
	.page-hero {
		display: grid;
		gap: var(--space-4);
	}

	.eyebrow {
		margin: 0;
		text-transform: uppercase;
		letter-spacing: 0.13em;
		font-size: var(--font-label);
		color: var(--muted);
		font-weight: 700;
	}

	h1 {
		margin: var(--space-2) 0;
		font-size: clamp(1.55rem, 3.2vw, 2.3rem);
		max-width: 24ch;
	}

	.lead {
		margin: 0;
		color: var(--muted);
		max-width: 68ch;
		line-height: 1.45;
	}

	.stamp {
		margin: 0;
		font-size: var(--font-tiny);
		color: var(--muted);
	}

	.actions {
		display: flex;
		gap: var(--space-2);
		flex-wrap: wrap;
	}
</style>
