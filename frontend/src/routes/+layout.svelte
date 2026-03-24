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
		justify-content: flex-end;
		padding: 0.8rem 1rem 0;
	}
</style>
