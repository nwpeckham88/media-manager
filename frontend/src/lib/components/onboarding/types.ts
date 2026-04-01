export type ApiState<T> = {
	ok: boolean;
	data?: T;
	error?: string;
};

export type AppConfigResponse = {
	library_roots: string[];
	state_dir: string;
	auth_enabled: boolean;
	metadata_provider: 'tmdb' | 'imdb' | 'tvdb';
	naming_format: 'movie_title_year' | 'movie_title_subtitle_year';
};

export type RootScanSummary = {
	root: string;
	exists: boolean;
	media_files: number;
	error: string | null;
};

export type ScanSummary = {
	roots: RootScanSummary[];
	total_media_files: number;
};

export type IndexStats = {
	total_indexed: number;
	hashed: number;
	probed: number;
	last_indexed_at_ms: number | null;
};

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

export type RecentJobsResponse = {
	total_count: number;
	offset: number;
	limit: number;
	items: JobRecord[];
};
