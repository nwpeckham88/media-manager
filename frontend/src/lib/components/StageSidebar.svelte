<script lang="ts">
	let { currentPath } = $props<{ currentPath: string }>();

	const utilityLinks = [
		{ href: '/queue', label: 'Queue' },
		{ href: '/operations', label: 'Operations' },
		{ href: '/library', label: 'Library (Advanced)' }
	];

	function isActive(pathname: string, href: string): boolean {
		return pathname === href || pathname.startsWith(`${href}/`);
	}
</script>

<section class="utility-shell" aria-label="Workflow Utilities">
	<div class="intro">
		<p class="mono heading">Utility Lanes</p>
		<p>Use Queue for live jobs, Operations for rollback history, and Library for manual inspections.</p>
	</div>
	<div class="utility-links" role="navigation" aria-label="Utility Routes">
		{#each utilityLinks as link}
			<a href={link.href} class:active={isActive(currentPath, link.href)}>{link.label}</a>
		{/each}
	</div>
</section>

<style>
	.utility-shell {
		margin: 0.65rem auto 0;
		width: min(1160px, 94vw);
		display: grid;
		gap: 0.65rem;
		padding: 0.75rem;
		border: 1px solid var(--ring);
		border-radius: 14px;
		background: color-mix(in srgb, var(--card) 90%, transparent);
		backdrop-filter: blur(3px);
	}

	.intro {
		display: flex;
		justify-content: space-between;
		gap: 0.7rem;
		align-items: baseline;
		flex-wrap: wrap;
	}

	.intro p {
		margin: 0;
		font-size: 0.84rem;
		color: var(--muted);
	}

	.heading {
		font-size: 0.74rem;
		text-transform: uppercase;
		letter-spacing: 0.12em;
		color: var(--muted);
		margin: 0;
	}

	.utility-links {
		display: flex;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	a {
		padding: 0.38rem 0.66rem;
		font-size: 0.8rem;
		text-decoration: none;
		border-radius: 999px;
		font-weight: 700;
		border: 1px solid var(--ring);
		background: color-mix(in srgb, var(--card) 85%, transparent);
	}

	a.active {
		border-color: color-mix(in srgb, var(--accent) 50%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 28%, transparent);
	}
</style>
