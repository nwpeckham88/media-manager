import type { PageLoad } from './$types';

type Branding = {
	app_name: string;
	short_name: string;
	logo_url: string;
	browser_title_template: string;
	theme_tokens: {
		accent: string;
		accent_contrast: string;
	};
};

type ToolchainSnapshot = {
	ffmpeg: {
		command_name: string;
		path: string;
		version_output: string | null;
		status: 'ok' | 'missing' | 'unavailable';
	};
	ffprobe: {
		command_name: string;
		path: string;
		version_output: string | null;
		status: 'ok' | 'missing' | 'unavailable';
	};
	mediainfo: {
		command_name: string;
		path: string;
		version_output: string | null;
		status: 'ok' | 'missing' | 'unavailable';
	} | null;
};

type ScanSummary = {
	roots: Array<{
		root: string;
		exists: boolean;
		media_files: number;
		error: string | null;
	}>;
	total_media_files: number;
};

type AppConfig = {
	library_roots: string[];
	state_dir: string;
	auth_enabled: boolean;
};

type OperationEvent = {
	timestamp_ms: number;
	kind: 'scan_summary' | 'sidecar_read' | 'sidecar_upsert';
	detail: string;
	success: boolean;
};

type JobRecord = {
	id: number;
	kind: string;
	status: 'running' | 'succeeded' | 'failed' | 'canceled';
	created_at_ms: number;
	updated_at_ms: number;
	payload_json: string;
	result_json: string | null;
	error: string | null;
};

type RecentJobsResponse = {
	total_count: number;
	offset: number;
	limit: number;
	items: JobRecord[];
};

type PreflightReport = {
	ready: boolean;
	checks: Array<{
		name: string;
		ok: boolean;
		detail: string;
	}>;
};

type ApiState<T> = {
	ok: boolean;
	data?: T;
	error?: string;
};

type LoadFetch = Parameters<PageLoad>[0]['fetch'];

export const load: PageLoad = async ({ fetch }) => {
	const [branding, toolchain, preflight, scanSummary, appConfig, recentOperations, recentJobs] = await Promise.all([
		readJson<Branding>(fetch, '/api/config/branding'),
		readJson<ToolchainSnapshot>(fetch, '/api/diagnostics/toolchain'),
		readJson<PreflightReport>(fetch, '/api/diagnostics/preflight'),
		readJson<ScanSummary>(fetch, '/api/scan/summary'),
		readJson<AppConfig>(fetch, '/api/config/app'),
		readJson<OperationEvent[]>(fetch, '/api/operations/recent?limit=12'),
		readJson<RecentJobsResponse>(fetch, '/api/jobs/recent?limit=12')
	]);

	return {
		branding,
		toolchain,
		preflight,
		scanSummary,
		appConfig,
		recentOperations,
		recentJobs,
		loadedAt: new Date().toISOString()
	};
};

async function readJson<T>(fetchFn: LoadFetch, path: string): Promise<ApiState<T>> {
	try {
		const token = typeof localStorage !== 'undefined' ? localStorage.getItem('mm-api-token') : null;
		const headers = token ? { Authorization: `Bearer ${token}` } : undefined;
		const response = await fetchFn(path, { headers });
		if (!response.ok) {
			return {
				ok: false,
				error: `HTTP ${response.status} from ${path}`
			};
		}
		const data = (await response.json()) as T;
		return { ok: true, data };
	} catch (error) {
		return {
			ok: false,
			error: error instanceof Error ? error.message : `Unknown error calling ${path}`
		};
	}
}
