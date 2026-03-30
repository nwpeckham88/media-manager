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
		gap: 0.6rem;
		padding: 0.55rem 0.65rem;
		border-radius: 10px;
		border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--ring));
		background: color-mix(in srgb, var(--accent) 12%, transparent);
		margin: 0.6rem 0;
		flex-wrap: wrap;
	}

	.result-banner.error-state {
		border-color: color-mix(in srgb, var(--danger) 40%, var(--ring));
		background: color-mix(in srgb, var(--danger) 10%, transparent);
	}

	.message {
		margin: 0;
		font-size: 0.82rem;
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
		border-radius: 8px;
		padding: 0.35rem 0.55rem;
		text-decoration: none;
		font-weight: 700;
		font-size: 0.82rem;
		background: color-mix(in srgb, var(--card) 95%, transparent);
	}
</style>
