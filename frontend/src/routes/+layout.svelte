<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import WorkflowProgress from '$lib/components/WorkflowProgress.svelte';
	import StageSidebar from '$lib/components/StageSidebar.svelte';
	import { isOnboardingComplete } from '$lib/workflow/onboarding';

	import '../app.css';
	import favicon from '$lib/assets/favicon.svg';

	let { children } = $props();
	let theme = $state<'light' | 'dark'>('light');
	let gateReady = $state(false);

	const onOnboardingRoute = $derived(page.url.pathname.startsWith('/onboarding'));
	const showAppChrome = $derived(gateReady && !onOnboardingRoute);

	onMount(() => {
		const saved = localStorage.getItem('mm-theme');
		if (saved === 'light' || saved === 'dark') {
			theme = saved;
		} else {
			theme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
		}

		applyTheme(theme);

		if (!isOnboardingComplete() && !page.url.pathname.startsWith('/onboarding')) {
			void goto('/onboarding');
		}

		gateReady = true;
	});

	$effect(() => {
		if (!gateReady) {
			return;
		}

		if (!isOnboardingComplete() && !page.url.pathname.startsWith('/onboarding')) {
			void goto('/onboarding');
		}
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

<div class="app-frame">
	<header class="theme-wrap">
		<div class="brand-wrap">
			<p class="mono product-name">Media Manager</p>
			<p class="product-tagline">Jellyfin-safe library automation</p>
		</div>

		{#if showAppChrome}
			<nav class="main-nav" aria-label="Primary">
				<a href="/" class:active={isActive(page.url.pathname, '/') && page.url.pathname === '/'}>Dashboard</a>
				<a href="/consolidation" class:active={isActive(page.url.pathname, '/consolidation')}>Consolidation</a>
				<a href="/metadata" class:active={isActive(page.url.pathname, '/metadata')}>Metadata</a>
				<a href="/formatting" class:active={isActive(page.url.pathname, '/formatting')}>Formatting</a>
				<a href="/queue" class:active={isActive(page.url.pathname, '/queue')}>Queue</a>
				<a href="/operations" class:active={isActive(page.url.pathname, '/operations')}>Operations</a>
			</nav>
		{/if}

		<button class="theme-toggle" type="button" onclick={toggleTheme}>
			Theme: {theme}
		</button>
	</header>

	{#if showAppChrome}
		<WorkflowProgress currentPath={page.url.pathname} />
		<StageSidebar currentPath={page.url.pathname} />
	{/if}

	{#if !gateReady && !onOnboardingRoute}
		<main class="gate-loader">
			<p class="mono">Preparing guided setup...</p>
		</main>
	{:else}
		<div class="page-slot" class:onboarding={onOnboardingRoute}>
			{@render children()}
		</div>
	{/if}
</div>

<style>
	.app-frame {
		min-height: 100vh;
	}

	.theme-wrap {
		position: sticky;
		top: 0;
		z-index: 20;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.9rem;
		padding: 0.75rem 1rem;
		background: color-mix(in srgb, var(--bg) 78%, transparent);
		backdrop-filter: blur(8px);
		border-bottom: 1px solid var(--ring);
	}

	.brand-wrap {
		display: grid;
		gap: 0.08rem;
	}

	.product-name {
		margin: 0;
		font-size: 0.78rem;
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-weight: 700;
		color: var(--muted);
	}

	.product-tagline {
		margin: 0;
		font-size: 0.88rem;
		font-weight: 700;
	}

	.main-nav {
		display: flex;
		gap: 0.55rem;
		flex-wrap: wrap;
		justify-content: center;
	}

	.main-nav a {
		padding: 0.42rem 0.72rem;
		border-radius: 999px;
		text-decoration: none;
		font-weight: 600;
		font-size: 0.84rem;
		background: color-mix(in srgb, var(--card) 90%, transparent);
		border: 1px solid var(--ring);
	}

	.main-nav a.active {
		border-color: color-mix(in srgb, var(--accent) 55%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 25%, transparent);
	}

	.gate-loader {
		width: min(920px, 94vw);
		margin: 1.2rem auto;
		padding: 1rem;
		border: 1px solid var(--ring);
		border-radius: 14px;
		background: color-mix(in srgb, var(--card) 92%, transparent);
	}

	.gate-loader p {
		margin: 0;
		font-size: 0.84rem;
		color: var(--muted);
	}

	.page-slot.onboarding {
		padding-top: 0.2rem;
	}

	@media (max-width: 1000px) {
		.theme-wrap {
			position: static;
			padding-top: 0.65rem;
			align-items: flex-start;
			gap: 0.6rem;
			flex-direction: column;
		}

		.main-nav {
			justify-content: flex-start;
		}
	}
</style>
