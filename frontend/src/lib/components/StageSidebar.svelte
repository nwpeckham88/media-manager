<script lang="ts">
	let { currentPath } = $props<{ currentPath: string }>();

	const utilityLinks = [
		{ href: '/queue', label: 'Queue' },
		{ href: '/operations', label: 'Operations' },
		{ href: '/settings', label: 'Settings' },
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
		margin: var(--space-3) auto 0;
		width: min(var(--content-max), 94vw);
		display: grid;
		gap: var(--space-3);
		padding: var(--space-4);
		border: 1px solid var(--ring);
		border-radius: var(--radius-lg);
		background: color-mix(in srgb, var(--card) 90%, transparent);
		backdrop-filter: blur(3px);
	}

	.intro {
		display: flex;
		justify-content: space-between;
		gap: var(--space-3);
		align-items: baseline;
		flex-wrap: wrap;
	}

	.intro p {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
	}

	.heading {
		font-size: var(--font-label);
		text-transform: uppercase;
		letter-spacing: 0.12em;
		color: var(--muted);
		margin: 0;
	}

	.utility-links {
		display: flex;
		gap: var(--space-2);
		flex-wrap: wrap;
	}

	a {
		padding: 0.38rem 0.66rem;
		font-size: var(--font-small);
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
