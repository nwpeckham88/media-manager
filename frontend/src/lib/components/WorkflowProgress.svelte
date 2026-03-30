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
	<p class="mono progress-label">
		Stage {activeStage.id}/4: {activeStage.label}
		<span>{activeStage.description}</span>
	</p>
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
		margin: 0.6rem 1rem 0;
		padding: 0.7rem;
		border: 1px solid var(--ring);
		border-radius: 14px;
		background: color-mix(in srgb, var(--card) 93%, transparent);
		backdrop-filter: blur(2px);
	}

	.progress-label {
		margin: 0 0 0.55rem;
		display: flex;
		gap: 0.5rem;
		align-items: baseline;
		font-size: 0.82rem;
	}

	.progress-label span {
		color: var(--muted);
		font-size: 0.75rem;
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
		padding: 0.45rem 0.55rem;
		border-radius: 10px;
		border: 1px solid var(--ring);
		text-decoration: none;
		font-size: 0.78rem;
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
	}
</style>
