<script lang="ts">
	import { onMount } from 'svelte';
	import OperationResultBanner from '$lib/components/OperationResultBanner.svelte';
	import PageHero from '$lib/components/ui/PageHero.svelte';
	import SurfaceCard from '$lib/components/ui/SurfaceCard.svelte';
	import { markStageComplete, markStageIncomplete } from '$lib/workflow/progress';
	import { apiFetch } from '$lib/utils/api';
	import type { BulkDryRunResponse, BulkApplyResponse, BulkRollbackResponse } from '$lib/types/api';

	type IndexStatsResponse = {
		total_indexed: number;
		hashed: number;
		probed: number;
		last_indexed_at_ms: number | null;
	};

	type ExactDuplicateItem = {
		media_path: string;
		file_size: number;
		parsed_title: string | null;
		parsed_year: number | null;
		parsed_provider_id: string | null;
		video_codec: string | null;
		audio_codec: string | null;
		width: number | null;
		height: number | null;
		duration_seconds: number | null;
	};

	type ExactDuplicateGroup = {
		content_hash: string;
		count: number;
		items: ExactDuplicateItem[];
	};

	type ExactDuplicatesResponse = {
		total_groups: number;
		groups: ExactDuplicateGroup[];
	};

	type SemanticDuplicateItem = {
		media_path: string;
		file_size: number;
		content_hash: string | null;
		video_codec: string | null;
		audio_codec: string | null;
		width: number | null;
		height: number | null;
	};

	type SemanticDuplicateGroup = {
		parsed_title: string;
		parsed_year: number | null;
		parsed_provider_id: string | null;
		item_count: number;
		variant_count: number;
		items: SemanticDuplicateItem[];
	};

	type SemanticDuplicatesResponse = {
		total_groups: number;
		groups: SemanticDuplicateGroup[];
	};

	type SemanticShowGroup = {
		key: string;
		parsed_title: string;
		parsed_year: number | null;
		parsed_provider_id: string | null;
		item_count: number;
		variant_count: number;
		bucket_count: number;
		items: SemanticDuplicateItem[];
	};

	type ConsolidationQuarantineResponse = {
		total_items: number;
		succeeded: number;
		failed: number;
		items: {
			media_path: string;
			success: boolean;
			operation_id: string | null;
		}[];
	};

	let loading = $state(false);
	let indexing = $state(false);
	let error = $state('');
	let notice = $state('');
	let stats = $state<IndexStatsResponse | null>(null);
	let exactGroups = $state<ExactDuplicateGroup[]>([]);
	let semanticShowGroups = $state<SemanticShowGroup[]>([]);
	let mergingKey = $state<string | null>(null);
	let quarantiningKey = $state<string | null>(null);
	let rollbacking = $state(false);
	let rollbackOperationIds = $state<string[]>([]);
	let rollbackResult = $state<BulkRollbackResponse | null>(null);
	let mergingAllShows = $state(false);
	let mergeAllProgress = $state('');
	let semanticPlanByKey = $state<
		Record<
			string,
			{
				loading: boolean;
				error: string | null;
				uid: string;
				mappings: { from: string; to: string; note: string }[];
			}
		>
	>({});
	let exactKeepModalOpen = $state(false);
	let modalGroup = $state<ExactDuplicateGroup | null>(null);
	let modalKeepPath = $state('');

	onMount(async () => {
		await refresh();
	});

	async function refresh() {
		loading = true;
		error = '';
		await Promise.all([loadStats(), loadExactDuplicateGroups(), loadSemanticDuplicateGroups()]);
		loading = false;
	}

	async function loadStats() {
		const response = await apiFetch('/api/index/stats');
		if (!response.ok) {
			error = `Unable to load index stats (${response.status})`;
			return;
		}

		stats = (await response.json()) as IndexStatsResponse;
	}

	async function loadExactDuplicateGroups() {
		const response = await apiFetch('/api/consolidation/exact-duplicates?limit=30&min_group_size=2');
		if (!response.ok) {
			error = `Unable to load duplicate groups (${response.status})`;
			return;
		}

		const payload = (await response.json()) as ExactDuplicatesResponse;
		exactGroups = payload.groups;
	}

	async function loadSemanticDuplicateGroups() {
		const response = await apiFetch('/api/consolidation/semantic-duplicates?limit=30&min_group_size=2');
		if (!response.ok) {
			error = `Unable to load semantic groups (${response.status})`;
			return;
		}

		const payload = (await response.json()) as SemanticDuplicatesResponse;
		semanticShowGroups = groupSemanticShowGroups(payload.groups);
	}

	function semanticGroupKey(group: SemanticDuplicateGroup): string {
		return `${group.parsed_title}|${group.parsed_year ?? 'none'}`;
	}

	function groupSemanticShowGroups(groups: SemanticDuplicateGroup[]): SemanticShowGroup[] {
		const map = new Map<string, SemanticShowGroup>();

		for (const group of groups) {
			const key = semanticGroupKey(group);
			let show = map.get(key);
			if (!show) {
				show = {
					key,
					parsed_title: group.parsed_title,
					parsed_year: group.parsed_year,
					parsed_provider_id: group.parsed_provider_id,
					item_count: 0,
					variant_count: 0,
					bucket_count: 0,
					items: []
				};
				map.set(key, show);
			} else if (show.parsed_provider_id !== group.parsed_provider_id) {
				show.parsed_provider_id = null;
			}

			show.bucket_count += 1;
			for (const item of group.items) {
				if (show.items.some((existing) => existing.media_path === item.media_path)) {
					continue;
				}
				show.items.push(item);
			}
		}

		const output = Array.from(map.values());
		for (const show of output) {
			show.item_count = show.items.length;
			show.variant_count = new Set(show.items.map((item) => item.content_hash ?? item.media_path)).size;
		}

		return output.sort((a, b) => b.item_count - a.item_count || a.parsed_title.localeCompare(b.parsed_title));
	}

	async function startIndexing() {
		indexing = true;
		error = '';
		notice = '';

		const response = await apiFetch('/api/index/start', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ include_hashes: true, include_probe: true })
		});
		if (!response.ok) {
			error = await response.text();
			indexing = false;
			return;
		}

		const payload = (await response.json()) as { job_id: number };
		notice = `Index started: job #${payload.job_id}. Next: monitor Queue.`;
		indexing = false;
		await refresh();
	}

	function canonicalUidForGroup(group: SemanticShowGroup): string {
		const base = group.parsed_title
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '');
		return `${base}${group.parsed_year ? `-${group.parsed_year}` : ''}`;
	}

	async function loadSemanticMergePlan(group: SemanticShowGroup) {
		const key = group.key;
		const uid = canonicalUidForGroup(group);
		semanticPlanByKey = {
			...semanticPlanByKey,
			[key]: { loading: true, error: null, uid, mappings: [] }
		};

		const itemsPayload = group.items.map((item) => ({
			media_path: item.media_path,
			item_uid: uid,
			rename_parent_folder: true,
			metadata_override: {
				title: group.parsed_title,
				year: group.parsed_year ?? undefined
			}
		}));

		const previewResponse = await apiFetch('/api/bulk/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'rename',
				items: itemsPayload
			})
		});

		if (!previewResponse.ok) {
			semanticPlanByKey = {
				...semanticPlanByKey,
				[key]: {
					loading: false,
					error: await previewResponse.text(),
					uid,
					mappings: []
				}
			};
			return;
		}

		const preview = (await previewResponse.json()) as BulkDryRunResponse;
		if (!preview.items) {
			semanticPlanByKey = {
				...semanticPlanByKey,
				[key]: {
					loading: false,
					error: 'Preview response missing items; try refresh and preview again.',
					uid,
					mappings: []
				}
			};
			return;
		}
		const mappings = preview.items
			.filter((item) => item.proposed_media_path && item.can_apply)
			.map((item) => ({
				from: item.media_path,
				to: item.proposed_media_path as string,
				note: item.note ?? ''
			}));

		semanticPlanByKey = {
			...semanticPlanByKey,
			[key]: {
				loading: false,
				error: preview.plan_ready ? null : 'Plan has invalid items; resolve before merge.',
				uid,
				mappings
			}
		};
	}

	async function mergeSemanticGroup(group: SemanticShowGroup): Promise<boolean> {
		if (group.items.length < 2) {
			return false;
		}

		const key = group.key;
		mergingKey = key;
		error = '';
		notice = '';

		const uid = canonicalUidForGroup(group);
		const itemsPayload = group.items.map((item) => ({
			media_path: item.media_path,
			item_uid: uid
		}));

		const previewResponse = await apiFetch('/api/bulk/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'combine_duplicates',
				items: itemsPayload
			})
		});
		if (!previewResponse.ok) {
			error = await previewResponse.text();
			mergingKey = null;
			return false;
		}

		const preview = (await previewResponse.json()) as BulkDryRunResponse;
		if (!preview.plan_ready) {
			error = 'Merge preview includes invalid items. Resolve them before apply.';
			mergingKey = null;
			return false;
		}

		const applyResponse = await apiFetch('/api/bulk/apply', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'combine_duplicates',
				approved_batch_hash: preview.batch_hash,
				items: itemsPayload
			})
		});
		if (!applyResponse.ok) {
			error = await applyResponse.text();
			mergingKey = null;
			return false;
		}

		const result = (await applyResponse.json()) as BulkApplyResponse;
		const operationIds = result.items
			.filter((item) => item.success && !!item.operation_id)
			.map((item) => item.operation_id as string);
		if (operationIds.length > 0) {
			rollbackOperationIds = [...rollbackOperationIds, ...operationIds];
		}
		let renameNote = '';
		if (result.succeeded > 0) {
			const renameOutcome = await normalizeSemanticGroupNames(group, uid);
			if (renameOutcome) {
				renameNote = ` | canonical rename ok=${renameOutcome.succeeded} fail=${renameOutcome.failed}`;
			}
		}

		if (result.failed === 0) {
			markStageComplete('consolidation');
		}
		notice = `Show merge complete: ${group.parsed_title} uid=${uid} (ok=${result.succeeded}, fail=${result.failed})${renameNote}.`;
		mergingKey = null;
		await refresh();
		return result.failed === 0;
	}

	async function mergeAllSemanticShows() {
		if (semanticShowGroups.length === 0 || mergingAllShows || mergingKey !== null) {
			return;
		}

		mergingAllShows = true;
		mergeAllProgress = '';
		error = '';
		notice = '';

		const total = semanticShowGroups.length;
		let index = 0;
		let succeeded = 0;

		for (const group of semanticShowGroups) {
			index += 1;
			mergeAllProgress = `Merging show ${index}/${total}: ${group.parsed_title}`;
			const ok = await mergeSemanticGroup(group);
			if (ok) {
				succeeded += 1;
			}
		}

		mergingAllShows = false;
		mergeAllProgress = '';
		if (error) {
			notice = `Merge-all partially complete: ok=${succeeded}, fail=${total - succeeded}.`;
		} else {
			notice = `Merge-all complete: processed ${total} show groups.`;
		}
	}

	async function normalizeSemanticGroupNames(group: SemanticShowGroup, uid: string): Promise<BulkApplyResponse | null> {
		const preferredTitle = group.parsed_title?.trim();
		if (!preferredTitle) {
			return null;
		}

		const itemsPayload = group.items.map((item) => ({
			media_path: item.media_path,
			item_uid: uid,
			rename_parent_folder: true,
			metadata_override: {
				title: preferredTitle,
				year: group.parsed_year ?? undefined
			}
		}));

		const previewResponse = await apiFetch('/api/bulk/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'rename',
				items: itemsPayload
			})
		});
		if (!previewResponse.ok) {
			error = `Rename preview failed: ${await previewResponse.text()}`;
			return null;
		}

		const preview = (await previewResponse.json()) as BulkDryRunResponse;
		if (!preview.plan_ready) {
			error = 'Rename preview contains invalid items. Resolve naming conflicts before retry.';
			return null;
		}

		const applyResponse = await apiFetch('/api/bulk/apply', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				action: 'rename',
				approved_batch_hash: preview.batch_hash,
				items: itemsPayload
			})
		});
		if (!applyResponse.ok) {
			error = `Rename apply failed: ${await applyResponse.text()}`;
			return null;
		}

		const applyResult = (await applyResponse.json()) as BulkApplyResponse;
		const operationIds = applyResult.items
			.filter((item) => item.success && !!item.operation_id)
			.map((item) => item.operation_id as string);
		if (operationIds.length > 0) {
			rollbackOperationIds = [...rollbackOperationIds, ...operationIds];
		}

		return applyResult;
	}

	function formatBytes(bytes: number): string {
		if (bytes < 1024) {
			return `${bytes} B`;
		}
		const kib = bytes / 1024;
		if (kib < 1024) {
			return `${kib.toFixed(1)} KiB`;
		}
		const mib = kib / 1024;
		if (mib < 1024) {
			return `${mib.toFixed(1)} MiB`;
		}
		return `${(mib / 1024).toFixed(2)} GiB`;
	}

	function exactGroupKey(group: ExactDuplicateGroup): string {
		return group.content_hash;
	}

	function openExactKeepModal(group: ExactDuplicateGroup) {
		if (group.items.length < 2) {
			return;
		}

		modalGroup = group;
		modalKeepPath = recommendedKeepPath(group);
		exactKeepModalOpen = true;
	}

	function closeExactKeepModal() {
		exactKeepModalOpen = false;
		modalGroup = null;
		modalKeepPath = '';
	}

	function qualityScore(item: ExactDuplicateItem): number {
		const pixelCount = (item.width ?? 0) * (item.height ?? 0);
		const duration = item.duration_seconds ?? 0;
		const sizeMib = item.file_size / (1024 * 1024);
		return pixelCount * 10 + duration * 4 + sizeMib;
	}

	function recommendedKeepPath(group: ExactDuplicateGroup): string {
		if (group.items.length === 0) {
			return '';
		}

		let best = group.items[0];
		let bestScore = qualityScore(best);
		for (const item of group.items.slice(1)) {
			const score = qualityScore(item);
			if (score > bestScore) {
				best = item;
				bestScore = score;
			}
		}

		return best.media_path;
	}

	function applyRecommendedKeepSelection() {
		if (!modalGroup) {
			return;
		}
		modalKeepPath = recommendedKeepPath(modalGroup);
	}

	async function quarantineExactGroup(group: ExactDuplicateGroup, keepMediaPath: string) {
		if (group.items.length < 2) {
			return;
		}

		const key = exactGroupKey(group);
		quarantiningKey = key;
		error = '';
		notice = '';

		const payload = {
			keep_media_path: keepMediaPath,
			media_paths: group.items.map((item) => item.media_path)
		};

		const response = await apiFetch('/api/consolidation/quarantine', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(payload)
		});
		if (!response.ok) {
			error = await response.text();
			quarantiningKey = null;
			return;
		}

		const result = (await response.json()) as ConsolidationQuarantineResponse;
		const operationIds = result.items
			.filter((item) => item.success && !!item.operation_id)
			.map((item) => item.operation_id as string);
		if (operationIds.length > 0) {
			rollbackOperationIds = [...rollbackOperationIds, ...operationIds];
		}
		if (result.failed === 0) {
			markStageComplete('consolidation');
		}
		notice = `Quarantine complete: hash ${group.content_hash.slice(0, 12)}... (ok=${result.succeeded}, fail=${result.failed}).`;
		quarantiningKey = null;
		await refresh();
	}

	async function confirmExactKeepSelection() {
		if (!modalGroup || !modalKeepPath) {
			return;
		}
		const group = modalGroup;
		const keepPath = modalKeepPath;
		closeExactKeepModal();
		await quarantineExactGroup(group, keepPath);
	}

	function formatDuration(seconds: number | null): string {
		if (seconds === null || !Number.isFinite(seconds)) {
			return 'unknown';
		}
		const total = Math.max(0, Math.round(seconds));
		const h = Math.floor(total / 3600);
		const m = Math.floor((total % 3600) / 60);
		const s = total % 60;
		if (h > 0) {
			return `${h}h ${m}m ${s}s`;
		}
		return `${m}m ${s}s`;
	}

	async function rollbackRecentOps() {
		if (rollbackOperationIds.length === 0) {
			error = 'No rollback operations are available in this session.';
			return;
		}

		rollbacking = true;
		error = '';
		notice = '';
		const response = await apiFetch('/api/bulk/rollback', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ operation_ids: rollbackOperationIds })
		});
		if (!response.ok) {
			error = await response.text();
			rollbacking = false;
			return;
		}

		rollbackResult = (await response.json()) as BulkRollbackResponse;
		notice = `Rollback complete: ok=${rollbackResult.succeeded}, fail=${rollbackResult.failed}.`;
		if (rollbackResult.failed === 0) {
			rollbackOperationIds = [];
			markStageIncomplete('consolidation');
		}
		rollbacking = false;
		await refresh();
	}

</script>

<svelte:head>
	<title>Media Manager | Consolidation</title>
</svelte:head>

<main class="stage-shell">
	<PageHero
		eyebrow="Stage 1"
		title="Consolidation"
		lead="Run library-wide indexing (hash + ffprobe), then review exact duplicate files before metadata and formatting passes."
	/>

	<section class="stage-card">
		<SurfaceCard as="div">
			<div class="actions">
			<button type="button" onclick={startIndexing} disabled={indexing || loading}>Start Full Index</button>
			<button type="button" onclick={refresh} disabled={loading}>Refresh</button>
			<button type="button" onclick={rollbackRecentOps} disabled={rollbacking || rollbackOperationIds.length === 0}>
				{rollbacking ? 'Rolling Back...' : 'Rollback Session Ops'}
			</button>
			<a class="queue-link" href="/queue">Queue</a>
			</div>
			<OperationResultBanner notice={notice} error={error} nextHref="/metadata" nextLabel="Next: Metadata" />

			{#if rollbackResult}
				<p class="mono summary-line">rollback total={rollbackResult.total_items} ok={rollbackResult.succeeded} fail={rollbackResult.failed}</p>
			{/if}

			{#if stats}
				<ul class="rows mono">
					<li><span>Indexed Files</span><strong>{stats.total_indexed}</strong></li>
					<li><span>Hashed Files</span><strong>{stats.hashed}</strong></li>
					<li><span>FFprobe Enriched</span><strong>{stats.probed}</strong></li>
					<li><span>Last Index</span><strong>{stats.last_indexed_at_ms ? new Date(stats.last_indexed_at_ms).toLocaleString() : 'never'}</strong></li>
				</ul>
			{/if}
		</SurfaceCard>
	</section>

	<section class="stage-card">
		<SurfaceCard as="div">
			<h2>Exact Duplicate Groups</h2>
			<p class="mono summary-line">Choose which file to keep for each exact-hash group. The rest are quarantined.</p>
		{#if loading}
			<p class="mono summary-line">Loading groups...</p>
		{:else if exactGroups.length === 0}
			<p class="mono summary-line">No exact duplicate groups detected yet.</p>
		{:else}
			<div class="group-grid">
				{#each exactGroups as group}
					{@const key = exactGroupKey(group)}
					<article class="group-card">
						<p class="mono">hash={group.content_hash.slice(0, 16)}... count={group.count}</p>
						<div class="actions">
							<button type="button" onclick={() => openExactKeepModal(group)} disabled={quarantiningKey !== null && quarantiningKey !== key}>
								{quarantiningKey === key ? 'Quarantining...' : 'Choose Keeper + Quarantine Rest'}
							</button>
						</div>
						<ul class="rows mono">
							{#each group.items as item}
								<li>
									<span>{item.media_path}</span>
									<strong>
										{formatBytes(item.file_size)}
										{item.width && item.height ? ` | ${item.width}x${item.height}` : ''}
										{item.video_codec ? ` | v:${item.video_codec}` : ''}
										{item.audio_codec ? ` | a:${item.audio_codec}` : ''}
									</strong>
								</li>
							{/each}
						</ul>
					</article>
				{/each}
			</div>
		{/if}
		</SurfaceCard>
	</section>

	<section class="stage-card">
		<SurfaceCard as="div">
			<h2>Semantic Duplicate Groups</h2>
			<p class="mono summary-line">Same parsed title/year/provider with multiple file variants.</p>
			<p class="mono summary-line">Use "Show Planned Result" to preview canonical UID and rename targets before merge.</p>
			<p class="mono merge-policy">
			Merge policy: canonical naming is mandatory. All matched items are normalized to
			<code>Movie Name - Subtitle (Year)</code> with spaces preserved and invalid characters replaced by <code>-</code>, and linked stem-matching
			metadata assets are renamed with the media file.
			</p>
			<div class="actions">
			<button type="button" onclick={mergeAllSemanticShows} disabled={loading || mergingAllShows || mergingKey !== null || semanticShowGroups.length === 0}>
				{mergingAllShows ? 'Merging All...' : 'Merge All Shows'}
			</button>
			{#if mergeAllProgress}
				<p class="mono merge-progress">{mergeAllProgress}</p>
			{/if}
			</div>
		{#if loading}
			<p class="mono summary-line">Loading semantic groups...</p>
		{:else if semanticShowGroups.length === 0}
			<p class="mono summary-line">No semantic duplicate groups detected yet.</p>
		{:else}
			<div class="group-grid">
				{#each semanticShowGroups as group}
					{@const key = group.key}
					<article class="group-card">
						<p class="mono">
							title={group.parsed_title} year={group.parsed_year ?? 'unknown'} provider={group.parsed_provider_id ?? 'mixed'}
						</p>
						<p class="mono">items={group.item_count} variants={group.variant_count} episode-buckets={group.bucket_count}</p>
						<div class="actions">
							<button type="button" onclick={() => loadSemanticMergePlan(group)} disabled={mergingKey !== null && mergingKey !== key}>
								Show Planned Result
							</button>
							<button type="button" onclick={() => mergeSemanticGroup(group)} disabled={mergingKey !== null && mergingKey !== key}>
								{mergingKey === key ? 'Merging...' : `Merge Show -> ${canonicalUidForGroup(group)}`}
							</button>
						</div>
						{#if semanticPlanByKey[key]}
							<div class="plan-box">
								<p class="mono">Planned UID: {semanticPlanByKey[key].uid}</p>
								{#if semanticPlanByKey[key].loading}
									<p class="mono muted">Loading merge plan...</p>
								{:else if semanticPlanByKey[key].error}
									<p class="error">{semanticPlanByKey[key].error}</p>
								{:else if semanticPlanByKey[key].mappings.length === 0}
									<p class="mono muted">No rename changes are required for current items.</p>
								{:else}
									<ul class="rows mono">
										{#each semanticPlanByKey[key].mappings.slice(0, 6) as mapping}
											<li>
												<span>{mapping.from}</span>
												<strong>{mapping.to}{mapping.note ? ` | ${mapping.note}` : ''}</strong>
											</li>
										{/each}
									</ul>
									{#if semanticPlanByKey[key].mappings.length > 6}
										<p class="mono muted">+{semanticPlanByKey[key].mappings.length - 6} more rename mappings</p>
									{/if}
								{/if}
							</div>
						{/if}
						<ul class="rows mono">
							{#each group.items as item}
								<li>
									<span>{item.media_path}</span>
									<strong>
										{formatBytes(item.file_size)}
										{item.width && item.height ? ` | ${item.width}x${item.height}` : ''}
										{item.video_codec ? ` | v:${item.video_codec}` : ''}
										{item.audio_codec ? ` | a:${item.audio_codec}` : ''}
									</strong>
								</li>
							{/each}
						</ul>
					</article>
				{/each}
			</div>
		{/if}
		</SurfaceCard>
	</section>

	{#if exactKeepModalOpen && modalGroup}
		<div class="modal-backdrop" role="presentation" onclick={(event) => event.target === event.currentTarget && closeExactKeepModal()}>
			<div class="modal" role="dialog" aria-modal="true" aria-labelledby="keep-title">
				<h3 id="keep-title">Select File To Keep</h3>
				<p class="mono modal-note">
					hash={modalGroup.content_hash.slice(0, 16)}... items={modalGroup.items.length}
				</p>
				<ul class="modal-list">
					{#each modalGroup.items as item}
						{@const recommended = item.media_path === recommendedKeepPath(modalGroup)}
						<li class:selected={modalKeepPath === item.media_path}>
							<label>
								<input type="radio" name="keep-media" value={item.media_path} bind:group={modalKeepPath} />
								<div>
									<p class="mono path">{item.media_path}</p>
									{#if recommended}
										<p class="recommended mono">Recommended keeper</p>
									{/if}
									<p class="meta">
										{formatBytes(item.file_size)}
										{item.width && item.height ? ` | ${item.width}x${item.height}` : ''}
										{item.video_codec ? ` | v:${item.video_codec}` : ''}
										{item.audio_codec ? ` | a:${item.audio_codec}` : ''}
										{` | duration:${formatDuration(item.duration_seconds)}`}
									</p>
									{#if item.parsed_title || item.parsed_year || item.parsed_provider_id}
										<p class="meta muted">
											{item.parsed_title ?? 'unknown title'}
											{item.parsed_year ? ` (${item.parsed_year})` : ''}
											{item.parsed_provider_id ? ` | ${item.parsed_provider_id}` : ''}
										</p>
									{/if}
								</div>
							</label>
						</li>
					{/each}
				</ul>
				<div class="actions">
					<button type="button" class="queue-link" onclick={applyRecommendedKeepSelection}>Use Recommended</button>
					<button type="button" class="queue-link" onclick={closeExactKeepModal}>Cancel</button>
					<button type="button" onclick={confirmExactKeepSelection} disabled={!modalKeepPath}>Keep Selected + Quarantine Rest</button>
				</div>
			</div>
		</div>
	{/if}
</main>

<style>
	.stage-shell {
		width: min(var(--content-max), 94vw);
		margin: var(--space-4) auto calc(var(--space-6) * 2);
		display: grid;
		gap: var(--space-4);
	}

	.stage-card {
		display: grid;
		gap: var(--space-3);
		backdrop-filter: blur(2px);
	}

	.actions {
		display: flex;
		gap: var(--space-2);
		align-items: center;
		flex-wrap: wrap;
		margin-bottom: var(--space-3);
	}

	button,
	.queue-link {
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		padding: 0.5rem 0.65rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
		font-weight: 700;
		text-decoration: none;
		font-size: var(--font-small);
	}

	button {
		cursor: pointer;
	}

	button:disabled {
		opacity: 0.62;
		cursor: not-allowed;
	}

	.summary-line {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
	}

	.rows {
		list-style: none;
		padding: 0;
		margin: 0;
		display: grid;
		gap: var(--space-2);
	}

	.rows li {
		display: flex;
		justify-content: space-between;
		gap: var(--space-3);
		border-bottom: 1px dashed var(--ring);
		padding-bottom: var(--space-2);
	}

	.group-grid {
		display: grid;
		gap: var(--space-3);
	}

	.group-card {
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: var(--space-3);
		background: color-mix(in srgb, var(--card) 87%, transparent);
	}

	.merge-policy {
		margin: var(--space-2) 0 var(--space-4);
		font-size: var(--font-body);
		color: var(--muted);
	}

	.merge-progress {
		margin: 0;
		color: var(--muted);
	}

	.plan-box {
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: var(--space-3);
		margin-bottom: var(--space-3);
		background: color-mix(in srgb, var(--card) 92%, transparent);
	}

	.plan-box p {
		margin: 0;
	}

	.plan-box .muted {
		color: var(--muted);
		margin-top: var(--space-2);
	}

	.modal-backdrop {
		position: fixed;
		inset: 0;
		z-index: 35;
		display: grid;
		place-items: center;
		padding: var(--space-4);
		background: color-mix(in srgb, #0b1218 48%, transparent);
		backdrop-filter: blur(3px);
	}

	.modal {
		width: min(980px, 94vw);
		max-height: 88vh;
		overflow: auto;
		border: 1px solid var(--ring);
		border-radius: var(--radius-lg);
		padding: var(--space-4);
		background: color-mix(in srgb, var(--card) 96%, transparent);
		display: grid;
		gap: var(--space-3);
	}

	.modal h3 {
		margin: 0;
	}

	.modal-note {
		margin: 0;
		font-size: var(--font-small);
		color: var(--muted);
	}

	.modal-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-2);
	}

	.modal-list li {
		border: 1px solid var(--ring);
		border-radius: var(--radius-md);
		padding: var(--space-3);
		background: color-mix(in srgb, var(--card) 92%, transparent);
	}

	.modal-list li.selected {
		border-color: color-mix(in srgb, var(--accent) 50%, var(--ring));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 28%, transparent);
	}

	.modal-list label {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-3);
		align-items: flex-start;
	}

	.path {
		margin: 0;
		font-size: var(--font-small);
		word-break: break-all;
	}

	.meta {
		margin: var(--space-1) 0 0;
		font-size: var(--font-small);
	}

	.meta.muted {
		color: var(--muted);
	}

	.recommended {
		margin: var(--space-1) 0 0;
		font-size: var(--font-label);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--accent);
	}

	@media (max-width: 760px) {
		button,
		.queue-link {
			width: 100%;
			text-align: center;
		}

		.rows li {
			flex-direction: column;
		}

		.modal-list label {
			grid-template-columns: 1fr;
		}
	}

</style>
