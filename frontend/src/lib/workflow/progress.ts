import { writable } from 'svelte/store';

export type WorkflowStage = 'consolidation' | 'metadata' | 'formatting' | 'verify';

export type WorkflowProgress = Record<WorkflowStage, boolean>;

export type WorkflowStageDescriptor = {
	id: number;
	key: WorkflowStage;
	label: string;
	path: string;
	description: string;
};

const STORAGE_KEY = 'mm-workflow-progress-v1';

const DEFAULT_WORKFLOW_PROGRESS: WorkflowProgress = {
	consolidation: false,
	metadata: false,
	formatting: false,
	verify: false
};

export const WORKFLOW_STAGES: WorkflowStageDescriptor[] = [
	{
		id: 1,
		key: 'consolidation',
		label: 'Consolidation',
		path: '/consolidation',
		description: 'Index + duplicate control'
	},
	{
		id: 2,
		key: 'metadata',
		label: 'Metadata',
		path: '/metadata',
		description: 'Provider IDs and confidence'
	},
	{
		id: 3,
		key: 'formatting',
		label: 'Formatting',
		path: '/formatting',
		description: 'Movie Name - Subtitle (Year) rename'
	},
	{
		id: 4,
		key: 'verify',
		label: 'Verify',
		path: '/queue',
		description: 'Audit and rollback'
	}
];

function isBrowser(): boolean {
	return typeof window !== 'undefined';
}

function normalizeProgress(value: unknown): WorkflowProgress {
	if (!value || typeof value !== 'object') {
		return { ...DEFAULT_WORKFLOW_PROGRESS };
	}

	const source = value as Partial<Record<WorkflowStage, unknown>>;
	return {
		consolidation: source.consolidation === true,
		metadata: source.metadata === true,
		formatting: source.formatting === true,
		verify: source.verify === true
	};
}

function readStoredProgress(): WorkflowProgress {
	if (!isBrowser()) {
		return { ...DEFAULT_WORKFLOW_PROGRESS };
	}

	const raw = window.localStorage.getItem(STORAGE_KEY);
	if (!raw) {
		return { ...DEFAULT_WORKFLOW_PROGRESS };
	}

	try {
		return normalizeProgress(JSON.parse(raw));
	} catch {
		return { ...DEFAULT_WORKFLOW_PROGRESS };
	}
}

function persistProgress(value: WorkflowProgress): void {
	if (!isBrowser()) {
		return;
	}

	window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
}

export const workflowProgress = writable<WorkflowProgress>(readStoredProgress());

if (isBrowser()) {
	workflowProgress.subscribe((value) => {
		persistProgress(value);
	});
}

export function mergeWorkflowProgress(progress: Partial<WorkflowProgress>): void {
	workflowProgress.update((current) => ({
		...current,
		...normalizeProgress({ ...current, ...progress })
	}));
}

export function markStageComplete(stage: WorkflowStage): void {
	workflowProgress.update((current) => ({
		...current,
		[stage]: true
	}));
}

export function markStageIncomplete(stage: WorkflowStage): void {
	workflowProgress.update((current) => ({
		...current,
		[stage]: false
	}));
}

export function workflowStageFromJobKind(kind: string): WorkflowStage | null {
	if (kind.includes('consolidation') || kind.includes('index')) {
		return 'consolidation';
	}
	if (kind.includes('metadata')) {
		return 'metadata';
	}
	if (kind.includes('rename') || kind.includes('formatting')) {
		return 'formatting';
	}
	if (kind.includes('rollback') || kind.includes('job')) {
		return 'verify';
	}
	return null;
}

export function workflowLabelFromJobKind(kind: string): string {
	const stage = workflowStageFromJobKind(kind);
	if (!stage) {
		return 'General';
	}

	return WORKFLOW_STAGES.find((entry) => entry.key === stage)?.label ?? 'General';
}

export function nextIncompleteStage(progress: WorkflowProgress): WorkflowStageDescriptor | null {
	for (const stage of WORKFLOW_STAGES) {
		if (!progress[stage.key]) {
			return stage;
		}
	}
	return null;
}
