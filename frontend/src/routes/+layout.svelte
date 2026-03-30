<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import WorkflowProgress from '$lib/components/WorkflowProgress.svelte';
	import StageSidebar from '$lib/components/StageSidebar.svelte';

	import '../app.css';
	import favicon from '$lib/assets/favicon.svg';

	let { children } = $props();
	let theme = $state<'light' | 'dark'>('light');

	onMount(() => {
		const saved = localStorage.getItem('mm-theme');
		if (saved === 'light' || saved === 'dark') {
			theme = saved;
		} else {
			theme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
		}

		applyTheme(theme);
	});

	function toggleTheme() {
		theme = theme === 'light' ? 'dark' : 'light';
		localStorage.setItem('mm-theme', theme);
		applyTheme(theme);
	}

	function applyTheme(selected: 'light' | 'dark') {
		document.documentElement.dataset.theme = selected;
	}

	function isActive(pathname: string, href: string): boolean {
		return pathname === href || pathname.startsWith(`${href}/`);
	}
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

<div class="theme-wrap">
	<nav class="main-nav" aria-label="Primary">
		<a href="/" class:active={isActive(page.url.pathname, '/') && page.url.pathname === '/'}>Dashboard</a>
		<a href="/consolidation" class:active={isActive(page.url.pathname, '/consolidation')}>Consolidation</a>
		<a href="/metadata" class:active={isActive(page.url.pathname, '/metadata')}>Metadata</a>
		<a href="/formatting" class:active={isActive(page.url.pathname, '/formatting')}>Formatting</a>
		<a href="/queue" class:active={isActive(page.url.pathname, '/queue')}>Queue</a>
		<a href="/operations" class:active={isActive(page.url.pathname, '/operations')}>Operations</a>
	</nav>
	<button class="theme-toggle" type="button" onclick={toggleTheme}>
		Theme: {theme}
	</button>
</div>

<WorkflowProgress currentPath={page.url.pathname} />
<StageSidebar currentPath={page.url.pathname} />

{@render children()}

<style>
	.theme-wrap {
		position: sticky;
		top: 0.6rem;
		z-index: 20;
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 0.8rem 1rem 0;
	}

	.main-nav {
		display: flex;
		gap: 0.7rem;
	}

	.main-nav a {
		padding: 0.4rem 0.75rem;
		border-radius: 999px;
		text-decoration: none;
		font-weight: 600;
		background: color-mix(in srgb, var(--card) 86%, transparent);
		border: 1px solid var(--ring);
	}

	.main-nav a.active {
		border-color: color-mix(in srgb, var(--accent) 55%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 25%, transparent);
	}

	@media (max-width: 1000px) {
		.theme-wrap {
			position: static;
			padding-top: 0.7rem;
			align-items: flex-start;
			gap: 0.6rem;
			flex-direction: column;
		}

		.main-nav {
			flex-wrap: wrap;
		}
	}
</style>
