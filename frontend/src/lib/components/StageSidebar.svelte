<script lang="ts">
	let { currentPath } = $props<{ currentPath: string }>();

	const utilityLinks = [
		{ href: '/operations', label: 'Operations' },
		{ href: '/library', label: 'Library (Advanced)' }
	];

	function isActive(pathname: string, href: string): boolean {
		return pathname === href || pathname.startsWith(`${href}/`);
	}
</script>

<section class="utility-shell" aria-label="Workflow Utilities">
	<p class="mono heading">Utilities</p>
	<div class="utility-links">
		{#each utilityLinks as link}
			<a href={link.href} class:active={isActive(currentPath, link.href)}>{link.label}</a>
		{/each}
	</div>
</section>

<style>
	.utility-shell {
		margin: 0.55rem 1rem 0;
		display: flex;
		align-items: center;
		gap: 0.8rem;
		flex-wrap: wrap;
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
		padding: 0.34rem 0.62rem;
		font-size: 0.8rem;
		text-decoration: none;
		border-radius: 999px;
		border: 1px dashed var(--ring);
		background: color-mix(in srgb, var(--card) 85%, transparent);
	}

	a.active {
		border-style: solid;
		border-color: color-mix(in srgb, var(--accent) 50%, var(--ring));
	}
</style>
