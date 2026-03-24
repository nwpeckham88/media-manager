<script lang="ts">
	let { data } = $props();

	type SidecarPlan = {
		plan_hash: string;
		media_path: string;
		sidecar_path: string;
		action: 'create' | 'update' | 'noop';
		next_state: {
			item_uid: string;
		};
	};

	type SidecarApplyResult = {
		operation_id: string;
		sidecar_path: string;
		applied_state: {
			item_uid: string;
		};
	};

	type SidecarRollbackResult = {
		operation_id: string;
		sidecar_path: string;
		restored: boolean;
	};

	type OperationEvent = {
		timestamp_ms: number;
		kind: string;
		detail: string;
		success: boolean;
	};

	type JobRecord = {
		id: number;
		kind: string;
		status: 'running' | 'succeeded' | 'failed';
		created_at_ms: number;
		updated_at_ms: number;
		payload_json: string;
		result_json: string | null;
		error: string | null;
	};

	let mediaPath = $state('');
	let itemUid = $state('');
	let apiToken = $state('');
	let dryRunPlan = $state<SidecarPlan | null>(null);
	let applyResult = $state<SidecarApplyResult | null>(null);
	let rollbackResult = $state<SidecarRollbackResult | null>(null);
	let workflowError = $state('');
	let busy = $state(false);
	let operations = $state<OperationEvent[]>([]);
	let jobs = $state<JobRecord[]>([]);

	$effect(() => {
		if (operations.length > 0) {
			return;
		}

		if (data.recentOperations.ok && data.recentOperations.data) {
			operations = data.recentOperations.data;
		}

		if (data.recentJobs.ok && data.recentJobs.data) {
			jobs = data.recentJobs.data;
		}
	});

	$effect(() => {
		const saved = localStorage.getItem('mm-api-token');
		if (saved && !apiToken) {
			apiToken = saved;
		}
	});

	async function runDryRun() {
		workflowError = '';
		rollbackResult = null;
		busy = true;
		try {
			const response = await apiFetch('/api/sidecar/dry-run', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ media_path: mediaPath, item_uid: itemUid })
			});
			if (!response.ok) {
				throw new Error(await response.text());
			}
			const payload = (await response.json()) as { plan: SidecarPlan };
			dryRunPlan = payload.plan;
			applyResult = null;
			await refreshOperations();
			await refreshJobs();
		} catch (error) {
			workflowError = error instanceof Error ? error.message : 'Dry-run failed';
		}
		busy = false;
	}

	async function applyPlan() {
		if (!dryRunPlan) {
			workflowError = 'Run dry-run first.';
			return;
		}

		workflowError = '';
		busy = true;
		try {
			const response = await apiFetch('/api/sidecar/apply', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({
					media_path: mediaPath,
					item_uid: itemUid,
					plan_hash: dryRunPlan.plan_hash
				})
			});
			if (!response.ok) {
				throw new Error(await response.text());
			}
			applyResult = (await response.json()) as SidecarApplyResult;
			await refreshOperations();
			await refreshJobs();
		} catch (error) {
			workflowError = error instanceof Error ? error.message : 'Apply failed';
		}
		busy = false;
	}

	async function rollbackLast() {
		if (!applyResult) {
			workflowError = 'No applied operation to rollback.';
			return;
		}

		workflowError = '';
		busy = true;
		try {
			const response = await apiFetch('/api/sidecar/rollback', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ operation_id: applyResult.operation_id })
			});
			if (!response.ok) {
				throw new Error(await response.text());
			}
			rollbackResult = (await response.json()) as SidecarRollbackResult;
			await refreshOperations();
			await refreshJobs();
		} catch (error) {
			workflowError = error instanceof Error ? error.message : 'Rollback failed';
		}
		busy = false;
	}

	async function refreshOperations() {
		const response = await apiFetch('/api/operations/recent?limit=12');
		if (!response.ok) {
			return;
		}
		operations = (await response.json()) as OperationEvent[];
	}

	async function refreshJobs() {
		const response = await apiFetch('/api/jobs/recent?limit=12');
		if (!response.ok) {
			return;
		}
		jobs = (await response.json()) as JobRecord[];
	}

	function saveToken() {
		if (apiToken.trim().length == 0) {
			localStorage.removeItem('mm-api-token');
			return;
		}
		localStorage.setItem('mm-api-token', apiToken.trim());
	}

	async function apiFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
		const headers = new Headers(init?.headers ?? {});
		if (apiToken.trim().length > 0) {
			headers.set('Authorization', `Bearer ${apiToken.trim()}`);
		}

		return fetch(input, {
			...init,
			headers
		});
	}
</script>

<svelte:head>
	<title>Media Manager | Implementation Slice</title>
</svelte:head>

<main class="shell">
	<section class="hero">
		<p class="eyebrow">Jellyfin-first automation</p>
		<h1>Media Manager</h1>
		<p class="lead">
			Phase-one dashboard showing runtime branding config and media toolchain diagnostics from the Rust API.
		</p>
		<p class="stamp mono">Loaded at {data.loadedAt}</p>
	</section>

	<section class="grid">
		<article class="card workflow-card">
			<h2>Runtime Security</h2>
			{#if data.appConfig.ok && data.appConfig.data}
				<p class="summary-figure mono">Auth enabled: {data.appConfig.data.auth_enabled ? 'yes' : 'no'}</p>
			{/if}
			<div class="field-grid">
				<label>
					<span>API Token (Bearer)</span>
					<input bind:value={apiToken} placeholder="paste token if MM_API_TOKEN is set" />
				</label>
			</div>
			<div class="actions">
				<button type="button" onclick={saveToken}>Save token</button>
			</div>
		</article>

		<article class="card">
			<h2>Preflight</h2>
			{#if data.preflight.ok && data.preflight.data}
				<p class="summary-figure mono">Ready: {data.preflight.data.ready ? 'yes' : 'no'}</p>
				<ul class="rows mono">
					{#each data.preflight.data.checks as check}
						<li>
							<span>{check.name}</span>
							<strong>{check.ok ? 'ok' : 'fail'}</strong>
						</li>
						<li class="detail-row"><span>{check.detail}</span><strong>check</strong></li>
					{/each}
				</ul>
			{:else}
				<p class="error">{data.preflight.error}</p>
			{/if}
		</article>

		<article class="card">
			<h2>Branding Config</h2>
			{#if data.branding.ok && data.branding.data}
				<ul class="rows">
					<li><span>App Name</span><strong>{data.branding.data.app_name}</strong></li>
					<li><span>Short Name</span><strong>{data.branding.data.short_name}</strong></li>
					<li><span>Logo URL</span><code>{data.branding.data.logo_url}</code></li>
					<li>
						<span>Accent</span>
						<strong class="chip" style={`--chip-bg:${data.branding.data.theme_tokens.accent};--chip-text:${data.branding.data.theme_tokens.accent_contrast}`}>
							{data.branding.data.theme_tokens.accent}
						</strong>
					</li>
				</ul>
			{:else}
				<p class="error">{data.branding.error}</p>
			{/if}
		</article>

		<article class="card">
			<h2>Toolchain Diagnostics</h2>
			{#if data.toolchain.ok && data.toolchain.data}
				<ul class="rows mono">
					<li>
						<span>{data.toolchain.data.ffmpeg.command_name}</span>
						<strong>{data.toolchain.data.ffmpeg.status}</strong>
					</li>
					<li><span>Path</span><code>{data.toolchain.data.ffmpeg.path}</code></li>
					<li><span>Version</span><code>{data.toolchain.data.ffmpeg.version_output ?? 'n/a'}</code></li>
					<li>
						<span>{data.toolchain.data.ffprobe.command_name}</span>
						<strong>{data.toolchain.data.ffprobe.status}</strong>
					</li>
					<li><span>Path</span><code>{data.toolchain.data.ffprobe.path}</code></li>
					<li><span>Version</span><code>{data.toolchain.data.ffprobe.version_output ?? 'n/a'}</code></li>
				</ul>
			{:else}
				<p class="error">{data.toolchain.error}</p>
			{/if}
		</article>

		<article class="card">
			<h2>Library Scan Summary</h2>
			{#if data.scanSummary.ok && data.scanSummary.data}
				<p class="summary-figure mono">{data.scanSummary.data.total_media_files} media files detected</p>
				<ul class="rows mono">
					{#if data.scanSummary.data.roots.length === 0}
						<li><span>Roots</span><strong>Set `MM_LIBRARY_ROOTS` to enable scanning</strong></li>
					{:else}
						{#each data.scanSummary.data.roots as root}
							<li>
								<span>{root.root}</span>
								<strong>{root.media_files} files</strong>
							</li>
						{/each}
					{/if}
				</ul>
			{:else}
				<p class="error">{data.scanSummary.error}</p>
			{/if}
		</article>

		<article class="card workflow-card">
			<h2>Sidecar Workflow</h2>
			{#if data.appConfig.ok && data.appConfig.data}
				<p class="summary-figure mono">State dir: {data.appConfig.data.state_dir}</p>
			{/if}

			<div class="field-grid">
				<label>
					<span>Media Path</span>
					<input bind:value={mediaPath} placeholder="/mnt/media/movie.mkv" />
				</label>
				<label>
					<span>Item UID</span>
					<input bind:value={itemUid} placeholder="movie-001" />
				</label>
			</div>

			<div class="actions">
				<button type="button" disabled={busy} onclick={runDryRun}>Dry-run</button>
				<button type="button" disabled={busy || !dryRunPlan} onclick={applyPlan}>Apply</button>
				<button type="button" disabled={busy || !applyResult} onclick={rollbackLast}>Rollback</button>
			</div>

			{#if dryRunPlan}
				<p class="mono result">Plan: {dryRunPlan.action} | hash {dryRunPlan.plan_hash}</p>
			{/if}
			{#if applyResult}
				<p class="mono result">Applied operation: {applyResult.operation_id}</p>
			{/if}
			{#if rollbackResult}
				<p class="mono result">Rollback restored: {rollbackResult.restored ? 'yes' : 'no'}</p>
			{/if}
			{#if workflowError}
				<p class="error">{workflowError}</p>
			{/if}
		</article>

		<article class="card">
			<h2>Recent Operations</h2>
			<ul class="rows mono">
				{#if operations.length === 0}
					<li><span>Feed</span><strong>No events yet</strong></li>
				{:else}
					{#each operations as event}
						<li>
							<span>{new Date(event.timestamp_ms).toLocaleTimeString()}</span>
							<strong>{event.kind}</strong>
						</li>
						<li class="detail-row"><span>{event.detail}</span><strong>{event.success ? 'ok' : 'fail'}</strong></li>
					{/each}
				{/if}
			</ul>
		</article>

		<article class="card">
			<h2>Recent Jobs</h2>
			<ul class="rows mono">
				{#if jobs.length === 0}
					<li><span>Jobs</span><strong>No jobs yet</strong></li>
				{:else}
					{#each jobs as job}
						<li>
							<span>#{job.id} {job.kind}</span>
							<strong>{job.status}</strong>
						</li>
						<li class="detail-row"><span>{new Date(job.updated_at_ms).toLocaleString()}</span><strong>{job.error ? 'error' : 'ok'}</strong></li>
					{/each}
				{/if}
			</ul>
		</article>
	</section>
</main>

<style>
	.shell {
		padding: 2rem 1rem 3rem;
		max-width: 980px;
		margin: 0 auto;
	}

	.hero {
		padding: 1.25rem;
		border: 1px solid var(--ring);
		background: color-mix(in srgb, var(--card) 88%, transparent);
		border-radius: 1.1rem;
		box-shadow: 0 18px 40px -28px rgba(31, 42, 47, 0.5);
		animation: rise 450ms ease-out;
	}

	.eyebrow {
		margin: 0;
		font-size: 0.72rem;
		letter-spacing: 0.18em;
		text-transform: uppercase;
		font-weight: 700;
		color: var(--muted);
	}

	h1 {
		margin: 0.35rem 0 0;
		font-size: clamp(2rem, 5vw, 3.4rem);
		line-height: 1;
	}

	.lead {
		margin: 0.75rem 0 0;
		max-width: 60ch;
		font-size: 1.05rem;
		color: var(--muted);
	}

	.stamp {
		margin: 1rem 0 0;
		font-size: 0.83rem;
		color: var(--muted);
	}

	.grid {
		display: grid;
		gap: 1rem;
		margin-top: 1rem;
		grid-template-columns: repeat(auto-fit, minmax(290px, 1fr));
	}

	.card {
		background: color-mix(in srgb, var(--card) 92%, transparent);
		padding: 1rem;
		border: 1px solid color-mix(in srgb, var(--ring) 80%, white);
		border-radius: 0.9rem;
		animation: rise 520ms ease-out;
	}

	h2 {
		margin: 0 0 0.6rem;
	}

	.rows {
		display: grid;
		gap: 0.6rem;
		padding: 0;
		margin: 0;
		list-style: none;
	}

	.rows li {
		display: flex;
		gap: 0.7rem;
		align-items: baseline;
		justify-content: space-between;
		border-bottom: 1px dashed color-mix(in srgb, var(--muted) 25%, transparent);
		padding-bottom: 0.4rem;
	}

	.rows span {
		color: var(--muted);
		font-size: 0.9rem;
	}

	.rows strong {
		font-weight: 700;
		text-transform: uppercase;
		font-size: 0.85rem;
	}

	code {
		font-size: 0.8rem;
		word-break: break-all;
		text-align: right;
	}

	.chip {
		padding: 0.2rem 0.5rem;
		border-radius: 999px;
		background: var(--chip-bg);
		color: var(--chip-text);
	}

	.error {
		margin: 0;
		padding: 0.55rem 0.7rem;
		border-radius: 0.65rem;
		background: color-mix(in srgb, var(--danger) 15%, white);
		color: #7c2d12;
		font-weight: 600;
	}

	.summary-figure {
		margin: 0 0 0.8rem;
		font-size: 0.95rem;
		font-weight: 600;
		color: var(--muted);
	}

	.workflow-card {
		grid-column: 1 / -1;
	}

	.field-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
		gap: 0.7rem;
	}

	label {
		display: grid;
		gap: 0.3rem;
		font-size: 0.9rem;
		color: var(--muted);
	}

	input {
		font: inherit;
		padding: 0.5rem 0.6rem;
		border-radius: 0.55rem;
		border: 1px solid color-mix(in srgb, var(--ring) 85%, transparent);
		background: color-mix(in srgb, var(--card) 85%, transparent);
		color: var(--text);
	}

	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.55rem;
		margin-top: 0.8rem;
	}

	button {
		font: inherit;
		font-weight: 700;
		padding: 0.45rem 0.75rem;
		border-radius: 999px;
		border: 1px solid var(--ring);
		background: color-mix(in srgb, var(--card) 88%, transparent);
		color: var(--text);
		cursor: pointer;
	}

	button:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.result {
		margin: 0.65rem 0 0;
		font-size: 0.85rem;
		color: var(--muted);
	}

	.detail-row span {
		font-size: 0.8rem;
	}

	@keyframes rise {
		from {
			opacity: 0;
			transform: translateY(12px);
		}
		to {
			opacity: 1;
			transform: translateY(0);
		}
	}
</style>
