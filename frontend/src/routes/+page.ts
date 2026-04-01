import type { PageLoad } from './$types';
import type {
	ApiState,
	DuplicateGroupsSummary,
	IndexItemsSummary,
	FormattingCandidatesSummary,
	GoldenStateProgress,
	RecentJobsResponse
} from '$lib/types/api';

type IndexStats = {
	total_indexed: number;
	hashed: number;
	probed: number;
	last_indexed_at_ms: number | null;
};

type LoadFetch = Parameters<PageLoad>[0]['fetch'];

export const load: PageLoad = async ({ fetch }) => {
	const [indexStats, exactDuplicates, semanticDuplicates, metadataQueue, formattingQueue, goldenStateProgress, recentJobs] = await Promise.all([
		readJson<IndexStats>(fetch, '/api/index/stats'),
		readJson<DuplicateGroupsSummary>(fetch, '/api/consolidation/exact-duplicates?limit=1&min_group_size=2'),
		readJson<DuplicateGroupsSummary>(fetch, '/api/consolidation/semantic-duplicates?limit=1&min_group_size=2'),
		readJson<IndexItemsSummary>(fetch, '/api/index/items?limit=1&offset=0&only_missing_provider=true&max_confidence=0.95'),
		readJson<FormattingCandidatesSummary>(fetch, '/api/formatting/candidates?limit=1&offset=0'),
		readJson<GoldenStateProgress>(fetch, '/api/workflow/golden-state-progress'),
		readJson<RecentJobsResponse>(fetch, '/api/jobs/recent?limit=12')
	]);

	return {
		indexStats,
		exactDuplicates,
		semanticDuplicates,
		metadataQueue,
		formattingQueue,
		goldenStateProgress,
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
