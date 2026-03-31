<script lang="ts">
	let { title, href = '', label = '' } = $props<{ title: string; href?: string; label?: string }>();
</script>

<div class="section-header">
	<h2>{title}</h2>
	{#if href && label}
		<a class="pill" href={href}>{label}</a>
	{/if}
</div>

<style>
	.section-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: var(--space-2);
		margin-bottom: var(--space-3);
	}

	.section-header h2 {
		margin: 0;
	}

	.pill {
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: 0.42rem 0.65rem;
		text-decoration: none;
		font-weight: 700;
		font-size: var(--font-small);
		background: color-mix(in srgb, var(--card) 95%, transparent);
	}

	@media (max-width: 760px) {
		.section-header {
			flex-direction: column;
			align-items: flex-start;
		}
	}
</style>
