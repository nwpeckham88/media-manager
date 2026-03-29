<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';

	type SidecarState = {
		item_uid: string;
		nfo_state: string;
		provider_ids: Record<string, string | null>;
	};

	type SidecarReadResponse = {
		sidecar_path: string;
		state: SidecarState | null;
	};

	type SidecarPlan = {
		plan_hash: string;
		action: 'create' | 'update' | 'noop';
		next_state: {
			item_uid: string;
		};
	};

	type SidecarApplyResponse = {
		operation_id: string;
		sidecar_path: string;
		applied_state: {
			item_uid: string;
		};
	};

	type SidecarRollbackResponse = {
		operation_id: string;
		sidecar_path: string;
		restored: boolean;
	};

	type ConfirmDialogState = {
		open: boolean;
		title: string;
		message: string;
		confirmLabel: string;
		busy: boolean;
	};

	let mediaPath = $state('');
	let itemUid = $state('');
	let sidecar = $state<SidecarReadResponse | null>(null);
	let plan = $state<SidecarPlan | null>(null);
	let applyResult = $state<SidecarApplyResponse | null>(null);
	let rollbackResult = $state<SidecarRollbackResponse | null>(null);
	let loading = $state(false);
	let busy = $state(false);
	let error = $state('');
	let confirmDialog = $state<ConfirmDialogState>({
		open: false,
		title: '',
		message: '',
		confirmLabel: 'Confirm',
		busy: false
	});
	let pendingConfirmAction = $state<null | (() => Promise<void>)>(null);

	onMount(async () => {
		const params = new URLSearchParams(window.location.search);
		mediaPath = params.get('media_path') ?? '';
		if (!mediaPath) {
			error = 'Missing media_path query parameter.';
			return;
		}
		await loadItem();
	});

	async function loadItem() {
		loading = true;
		error = '';
		const response = await apiFetch(`/api/sidecar?media_path=${encodeURIComponent(mediaPath)}`);
		if (!response.ok) {
			error = await response.text();
			loading = false;
			return;
		}

		sidecar = (await response.json()) as SidecarReadResponse;
		itemUid = sidecar.state?.item_uid ?? deriveItemUidFromPath(mediaPath);
		loading = false;
	}

	async function runDryRun() {
		busy = true;
		error = '';
		const response = await apiFetch('/api/sidecar/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ media_path: mediaPath, item_uid: itemUid })
		});
		if (!response.ok) {
			error = await response.text();
			busy = false;
			return;
		}

		const payload = (await response.json()) as { plan: SidecarPlan };
		plan = payload.plan;
		busy = false;
	}

	async function applyPlan() {
		if (!plan) {
			error = 'Run dry-run first.';
			return;
		}

		busy = true;
		error = '';
		const response = await apiFetch('/api/sidecar/apply', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ media_path: mediaPath, item_uid: itemUid, plan_hash: plan.plan_hash })
		});
		if (!response.ok) {
			error = await response.text();
			busy = false;
			return;
		}

		applyResult = (await response.json()) as SidecarApplyResponse;
		busy = false;
		await loadItem();
	}

	function openConfirmDialog(title: string, message: string, confirmLabel: string, action: () => Promise<void>) {
		confirmDialog = {
			open: true,
			title,
			message,
			confirmLabel,
			busy: false
		};
		pendingConfirmAction = action;
	}

	function closeConfirmDialog() {
		if (confirmDialog.busy) {
			return;
		}

		confirmDialog = { ...confirmDialog, open: false };
		pendingConfirmAction = null;
	}

	async function runConfirmDialogAction() {
		if (!pendingConfirmAction) {
			return;
		}

		confirmDialog = { ...confirmDialog, busy: true };
		try {
			await pendingConfirmAction();
		} finally {
			confirmDialog = { ...confirmDialog, open: false, busy: false };
			pendingConfirmAction = null;
		}
	}

	function rollbackLast() {
		if (!applyResult) {
			error = 'No apply result to rollback.';
			return;
		}

		openConfirmDialog(
			'Rollback last operation?',
			`This restores operation ${applyResult.operation_id} if rollback data is still available.`,
			'Rollback',
			performRollbackLast
		);
	}

	async function performRollbackLast() {
		if (!applyResult) {
			error = 'No apply result to rollback.';
			return;
		}

		busy = true;
		error = '';
		const response = await apiFetch('/api/sidecar/rollback', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ operation_id: applyResult.operation_id })
		});
		if (!response.ok) {
			error = await response.text();
			busy = false;
			return;
		}

		rollbackResult = (await response.json()) as SidecarRollbackResponse;
		busy = false;
		await loadItem();
	}

	function deriveItemUidFromPath(path: string): string {
		const file = path.split('/').pop() ?? path;
		return file.replace(/\.[^.]+$/, '');
	}

	async function apiFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
		const headers = new Headers(init?.headers ?? {});
		const token = localStorage.getItem('mm-api-token');
		if (token && token.trim().length > 0) {
			headers.set('Authorization', `Bearer ${token.trim()}`);
		}

		return fetch(input, {
			...init,
			headers
		});
	}
</script>

<svelte:head>
	<title>Media Manager | Item Detail</title>
</svelte:head>

<main class="detail-shell">
	<section class="hero">
		<p class="eyebrow">Item inspection workflow</p>
		<h1>Media Item</h1>
		<p class="mono break">{mediaPath || 'No media path'}</p>
		<p><a class="back-link" href="/library">Back to Library</a></p>
	</section>

	<section class="card">
		{#if loading}
			<p class="mono">Loading item details...</p>
		{:else if sidecar}
			<ul class="rows mono">
				<li><span>Sidecar path</span><strong>{sidecar.sidecar_path}</strong></li>
				<li><span>Item UID</span><strong>{sidecar.state?.item_uid ?? 'none'}</strong></li>
				<li><span>NFO state</span><strong>{sidecar.state?.nfo_state ?? 'unknown'}</strong></li>
			</ul>
		{:else}
			<p class="mono">No sidecar details loaded.</p>
		{/if}
	</section>

	<section class="card">
		<label>
			<span>Item UID</span>
			<input bind:value={itemUid} placeholder="item uid" />
		</label>
		<div class="actions">
			<button type="button" disabled={busy} onclick={runDryRun}>Dry-run</button>
			<button type="button" disabled={busy || !plan} onclick={applyPlan}>Apply</button>
			<button type="button" disabled={busy || !applyResult} onclick={rollbackLast}>Rollback</button>
		</div>

		{#if plan}
			<p class="mono">Plan action={plan.action} hash={plan.plan_hash}</p>
		{/if}
		{#if applyResult}
			<p class="mono">Applied operation: {applyResult.operation_id}</p>
		{/if}
		{#if rollbackResult}
			<p class="mono">Rollback restored: {rollbackResult.restored ? 'yes' : 'no'}</p>
		{/if}
		{#if error}
			<p class="error">{error}</p>
		{/if}
	</section>

	<ConfirmDialog
		open={confirmDialog.open}
		title={confirmDialog.title}
		message={confirmDialog.message}
		confirmLabel={confirmDialog.confirmLabel}
		busy={confirmDialog.busy}
		tone="danger"
		onCancel={closeConfirmDialog}
		onConfirm={runConfirmDialogAction}
	/>
</main>

<style>
	.detail-shell {
		width: min(1080px, 92vw);
		margin: 0 auto 3rem;
		display: grid;
		gap: 1rem;
	}

	.hero {
		padding: 0.4rem 0;
	}

	.eyebrow {
		text-transform: uppercase;
		letter-spacing: 0.12em;
		font-size: 0.78rem;
		color: var(--muted);
		font-weight: 700;
	}

	.card {
		background: color-mix(in srgb, var(--card) 92%, transparent);
		border: 1px solid var(--ring);
		border-radius: 14px;
		padding: 1rem;
		backdrop-filter: blur(2px);
	}

	.rows {
		list-style: none;
		padding: 0;
		margin: 0;
		display: grid;
		gap: 0.45rem;
	}

	.rows li {
		display: flex;
		justify-content: space-between;
		gap: 0.7rem;
		border-bottom: 1px dashed var(--ring);
		padding-bottom: 0.3rem;
	}

	label {
		display: grid;
		gap: 0.3rem;
		min-width: 300px;
	}

	label span {
		font-size: 0.85rem;
		color: var(--muted);
	}

	input,
	button {
		border-radius: 10px;
		border: 1px solid var(--ring);
		padding: 0.5rem 0.65rem;
		font: inherit;
		background: color-mix(in srgb, var(--card) 95%, transparent);
		color: var(--text);
	}

	.actions {
		display: flex;
		gap: 0.7rem;
		margin-top: 0.7rem;
	}

	button {
		cursor: pointer;
		font-weight: 600;
	}

	button:disabled {
		opacity: 0.55;
		cursor: not-allowed;
	}

	.back-link {
		display: inline-flex;
		padding: 0.4rem 0.75rem;
		border: 1px solid var(--ring);
		border-radius: 999px;
		text-decoration: none;
		font-weight: 700;
	}

	.break {
		word-break: break-all;
	}

	.error {
		color: var(--danger);
		font-weight: 700;
	}

	@media (max-width: 900px) {
		.detail-shell {
			width: min(100%, 96vw);
		}
	}
</style>
