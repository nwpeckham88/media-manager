<script lang="ts">
	import OnboardingWizard from '$lib/components/onboarding/OnboardingWizard.svelte';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
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
	<PageHero
		eyebrow="First Boot Guide"
		title="Prepare Your Jellyfin Library for Safe Automation"
		lead="This guided setup confirms library detection, captures your metadata and naming golden state, then starts indexing only at the final launch step."
	/>

	<section class="wizard-card">
		<SurfaceCard as="div">
			<OnboardingWizard
				configState={data.configState}
				scanState={data.scanState}
				indexStatsState={data.indexStatsState}
				recentJobsState={data.recentJobsState}
			/>
		</SurfaceCard>
	</section>
</main>

<style>
	.onboarding-page {
		width: min(var(--content-max), 94vw);
		margin: var(--space-5) auto calc(var(--space-6) * 2);
		display: grid;
		gap: var(--space-4);
		animation: reveal 260ms ease-out;
	}

	.wizard-card {
		display: grid;
		gap: var(--space-3);
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
