import type { PageLoad } from './$types';
import type {
	ApiState,
	AppConfigResponse,
	IndexStats,
	RecentJobsResponse,
	ScanSummary
} from '$lib/components/onboarding/types';

type LoadFetch = Parameters<PageLoad>[0]['fetch'];

export const load: PageLoad = async ({ fetch }) => {
	const [configState, scanState, indexStatsState, recentJobsState] = await Promise.all([
		readJson<AppConfigResponse>(fetch, '/api/config/app'),
		readJson<ScanSummary>(fetch, '/api/scan/summary'),
		readJson<IndexStats>(fetch, '/api/index/stats'),
		readJson<RecentJobsResponse>(fetch, '/api/jobs/recent?limit=10')
	]);

	return {
		configState,
		scanState,
		indexStatsState,
		recentJobsState
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
