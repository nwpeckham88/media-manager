<script lang="ts">
	import { onMount } from 'svelte';

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
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

<div class="theme-wrap">
	<nav class="main-nav" aria-label="Primary">
		<a href="/">Dashboard</a>
		<a href="/library">Library</a>
		<a href="/queue">Queue</a>
	</nav>
	<button class="theme-toggle" type="button" onclick={toggleTheme}>
		Theme: {theme}
	</button>
</div>

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
</style>
