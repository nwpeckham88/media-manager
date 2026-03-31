<script lang="ts">
	let {
		notice = '',
		error = '',
		nextHref = '',
		nextLabel = ''
	} = $props<{
		notice?: string;
		error?: string;
		nextHref?: string;
		nextLabel?: string;
	}>();
</script>

{#if notice || error}
	<section class="result-banner" class:error-state={!!error} aria-live="polite">
		{#if error}
			<p class="message error mono">{error}</p>
		{:else}
			<p class="message notice mono">{notice}</p>
			{#if nextHref && nextLabel}
				<a class="next-link" href={nextHref}>{nextLabel}</a>
			{/if}
		{/if}
	</section>
{/if}

<style>
	.result-banner {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-md);
		border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--ring));
		background: color-mix(in srgb, var(--accent) 12%, transparent);
		margin: var(--space-3) 0;
		flex-wrap: wrap;
	}

	.result-banner.error-state {
		border-color: color-mix(in srgb, var(--danger) 40%, var(--ring));
		background: color-mix(in srgb, var(--danger) 10%, transparent);
	}

	.message {
		margin: 0;
		font-size: var(--font-small);
	}

	.error {
		color: var(--danger);
		font-weight: 700;
	}

	.notice {
		color: var(--text);
	}

	.next-link {
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: 0.35rem 0.55rem;
		text-decoration: none;
		font-weight: 700;
		font-size: var(--font-small);
		background: color-mix(in srgb, var(--card) 95%, transparent);
	}
</style>
