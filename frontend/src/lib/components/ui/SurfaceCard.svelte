<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		as = 'article',
		tone = 'default',
		compact = false,
		className = '',
		children
	} = $props<{
		as?: keyof HTMLElementTagNameMap;
		tone?: 'default' | 'hero';
		compact?: boolean;
		className?: string;
		children?: Snippet;
	}>();
</script>

<svelte:element
	this={as}
	class={`surface-card tone-${tone} ${compact ? 'is-compact' : ''} ${className}`.trim()}
>
	{@render children?.()}
</svelte:element>

<style>
	.surface-card {
		border: 1px solid var(--ring);
		border-radius: var(--radius-lg);
		padding: var(--space-5);
		background: color-mix(in srgb, var(--card) 92%, transparent);
		box-shadow: var(--shadow);
	}

	.surface-card.is-compact {
		padding: var(--space-4);
	}

	.surface-card.tone-hero {
		background:
			radial-gradient(circle at 88% 8%, color-mix(in srgb, var(--accent) 24%, transparent), transparent 40%),
			color-mix(in srgb, var(--card) 94%, transparent);
	}
</style>
