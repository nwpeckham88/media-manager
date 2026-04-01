// Shared API types used across multiple pages.

export type JobRecord = {
	id: number;
	kind: string;
	status: 'running' | 'succeeded' | 'failed' | 'canceled';
	created_at_ms: number;
	updated_at_ms: number;
	payload_json: string;
	result_json: string | null;
	error: string | null;
};

export type ApiState<T> = {
	ok: boolean;
	data?: T;
	error?: string;
};

export type BulkAction = 'metadata_lookup' | 'combine_duplicates' | 'rename' | 'validate_nfo';

export const BULK_ACTION_METADATA_LOOKUP: BulkAction = 'metadata_lookup';
export const BULK_ACTION_COMBINE_DUPLICATES: BulkAction = 'combine_duplicates';
export const BULK_ACTION_RENAME: BulkAction = 'rename';
export const BULK_ACTION_VALIDATE_NFO: BulkAction = 'validate_nfo';

export type DuplicateGroupsSummary = {
	total_groups: number;
};

export type IndexItemsSummary = {
	total_items: number;
};

export type FormattingCandidatesSummary = {
	total_items: number;
};

export type GoldenStateProgress = {
	metadata_provider: 'tmdb' | 'imdb' | 'tvdb';
	naming_format: 'movie_title_year' | 'movie_title_subtitle_year';
	total_indexed: number;
	metadata_non_compliant: number;
	naming_non_compliant: number;
};

export type RecentJobsResponse = {
	items: JobRecord[];
};

export type BulkDryRunResponse = {
	action?: BulkAction;
	batch_hash: string;
	total_items: number;
	plan_ready: boolean;
	summary: {
		creates?: number;
		updates?: number;
		noops?: number;
		invalid: number;
	};
	items?: Array<{
		media_path: string;
		item_uid?: string;
		plan?: {
			plan_hash: string;
			action: 'create' | 'update' | 'noop';
			sidecar_path: string;
		} | null;
		proposed_media_path: string | null;
		proposed_item_uid?: string | null;
		proposed_provider_id?: string | null;
		metadata_title?: string | null;
		metadata_year?: number | null;
		metadata_confidence?: number | null;
		can_apply: boolean;
		note: string | null;
		error: string | null;
	}>;
};

export type BulkApplyResponse = {
	action?: BulkAction;
	batch_hash?: string;
	total_items: number;
	succeeded: number;
	failed: number;
	items: Array<{
		media_path: string;
		final_media_path?: string | null;
		item_uid?: string;
		applied_provider_id?: string | null;
		success: boolean;
		operation_id: string | null;
		error: string | null;
	}>;
};

export type BulkRollbackResponse = {
	total_items: number;
	succeeded: number;
	failed: number;
	items?: Array<{
		operation_id: string;
		success: boolean;
		detail: string | null;
		error: string | null;
	}>;
};

export type ConfirmDialogState = {
	open: boolean;
	title: string;
	message: string;
	confirmLabel: string;
	tone?: 'default' | 'danger';
	busy: boolean;
};
