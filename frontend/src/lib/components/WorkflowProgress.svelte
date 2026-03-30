<script lang="ts">
	import { onMount } from 'svelte';
	import {
		nextIncompleteStage,
		WORKFLOW_STAGES,
		workflowProgress,
		type WorkflowProgress as WorkflowProgressState
	} from '$lib/workflow/progress';

	let { currentPath } = $props<{ currentPath: string }>();
	let stageProgress = $state<WorkflowProgressState>({
		consolidation: false,
		metadata: false,
		formatting: false,
		verify: false
	});

	onMount(() => {
		const unsubscribe = workflowProgress.subscribe((value) => {
			stageProgress = value;
		});

		return unsubscribe;
	});

	function stageIndex(pathname: string): number {
		const idx = WORKFLOW_STAGES.findIndex((stage) => pathname.startsWith(stage.path));
		if (idx >= 0) {
			return idx;
		}

		const nextStage = nextIncompleteStage(stageProgress);
		if (!nextStage) {
			return WORKFLOW_STAGES.length - 1;
		}

		return Math.max(
			0,
			WORKFLOW_STAGES.findIndex((stage) => stage.key === nextStage.key)
		);
	}

	const activeIndex = $derived(stageIndex(currentPath));
	const activeStage = $derived(WORKFLOW_STAGES[activeIndex]);
</script>

<section class="workflow-strip" aria-label="Workflow Progress">
	<div class="progress-head">
		<p class="mono progress-label">Stage {activeStage.id}/4: {activeStage.label}</p>
		<p>{activeStage.description}</p>
	</div>
	<div class="track">
		{#each WORKFLOW_STAGES as stage, idx}
			<a
				href={stage.path}
				class:complete={stageProgress[stage.key]}
				class:active={idx === activeIndex}
			>
				<span>{stage.id}</span>
				<strong>{stage.label}</strong>
			</a>
		{/each}
	</div>
</section>

<style>
	.workflow-strip {
		width: min(1160px, 94vw);
		margin: 0.7rem auto 0;
		padding: 0.8rem;
		border: 1px solid var(--ring);
		border-radius: 16px;
		background: color-mix(in srgb, var(--card) 93%, transparent);
		backdrop-filter: blur(4px);
	}

	.progress-head {
		display: flex;
		justify-content: space-between;
		gap: 0.6rem;
		align-items: baseline;
		flex-wrap: wrap;
		margin-bottom: 0.55rem;
	}

	.progress-head p {
		margin: 0;
		font-size: 0.84rem;
		color: var(--muted);
	}

	.progress-label {
		font-weight: 700;
		color: var(--text);
	}

	.track {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 0.45rem;
	}

	a {
		display: flex;
		align-items: center;
		gap: 0.45rem;
		padding: 0.5rem 0.6rem;
		border-radius: 10px;
		border: 1px solid var(--ring);
		text-decoration: none;
		font-size: 0.8rem;
		font-weight: 700;
		background: color-mix(in srgb, var(--card) 90%, transparent);
	}

	a span {
		display: inline-grid;
		place-items: center;
		width: 1.2rem;
		height: 1.2rem;
		border-radius: 999px;
		font-weight: 700;
		font-size: 0.72rem;
		background: color-mix(in srgb, var(--card) 75%, var(--bg-alt));
	}

	a.active {
		border-color: color-mix(in srgb, var(--accent) 60%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 25%, transparent);
	}

	a.complete span {
		background: var(--accent);
		color: var(--accent-contrast);
	}

	@media (max-width: 860px) {
		.track {
			grid-template-columns: 1fr 1fr;
		}

		.progress-head {
			align-items: flex-start;
		}
	}
</style>
