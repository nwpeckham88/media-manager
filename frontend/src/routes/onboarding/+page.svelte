<script lang="ts">
	import OnboardingWizard from '$lib/components/onboarding/OnboardingWizard.svelte';
	import type {
		ApiState,
		AppConfigResponse,
		IndexStats,
		RecentJobsResponse,
		ScanSummary
	} from '$lib/components/onboarding/types';

	let { data } = $props<{
		data: {
			configState: ApiState<AppConfigResponse>;
			scanState: ApiState<ScanSummary>;
			indexStatsState: ApiState<IndexStats>;
			recentJobsState: ApiState<RecentJobsResponse>;
		};
	}>();
</script>

<svelte:head>
	<title>Media Manager | First Boot Setup</title>
</svelte:head>

<main class="onboarding-page">
	<section class="hero">
		<p class="eyebrow">First Boot Guide</p>
		<h1>Prepare Your Jellyfin Library for Safe Automation</h1>
		<p>
			This guided setup confirms library detection, kicks off indexing, and applies opinionated defaults for
			portable naming.
		</p>
	</section>

	<section class="wizard-card">
		<OnboardingWizard
			configState={data.configState}
			scanState={data.scanState}
			indexStatsState={data.indexStatsState}
			recentJobsState={data.recentJobsState}
		/>
	</section>
</main>

<style>
	.onboarding-page {
		width: min(1000px, 94vw);
		margin: 1.2rem auto 3rem;
		display: grid;
		gap: 1rem;
		animation: reveal 260ms ease-out;
	}

	.hero {
		border: 1px solid var(--ring);
		border-radius: 18px;
		padding: 1.1rem;
		background:
			radial-gradient(circle at 82% 12%, color-mix(in srgb, var(--accent) 24%, transparent), transparent 44%),
			color-mix(in srgb, var(--card) 92%, transparent);
	}

	.eyebrow {
		margin: 0;
		text-transform: uppercase;
		letter-spacing: 0.13em;
		font-size: 0.72rem;
		font-weight: 700;
		color: var(--muted);
	}

	h1 {
		margin: 0.35rem 0;
		font-size: clamp(1.5rem, 3.8vw, 2.2rem);
		max-width: 24ch;
	}

	.hero p {
		margin: 0;
		max-width: 66ch;
		color: var(--muted);
	}

	.wizard-card {
		border: 1px solid var(--ring);
		border-radius: 18px;
		padding: 1rem;
		background: color-mix(in srgb, var(--card) 90%, transparent);
	}

	@keyframes reveal {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
		to {
			opacity: 1;
			transform: translateY(0);
		}
	}
</style>
