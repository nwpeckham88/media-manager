<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';

	type SidecarState = {
		item_uid: string;
		nfo_state: string;
		provider_ids: Record<string, string | null>;
		preferred_policy_state: unknown;
	};

	type DesiredMediaState = {
		container: 'mkv' | 'mp4';
		video: {
			preferred_codec: 'av1' | 'hevc' | 'h264';
			minimum_allowed_codec: 'hevc' | 'h264';
			allow_manual_codec_upgrade: boolean;
		};
		audio: {
			allowed_layouts: Array<'stereo' | 'surround51' | 'surround71'>;
			require_stereo_track: boolean;
			add_night_mode_stereo_track: boolean;
			transcode_stereo_to_opus: boolean;
			transcode_standard_surround_to_opus: boolean;
			preserve_object_audio_tracks: boolean;
			night_mode_target_lufs: number;
		};
		subtitles: {
			keep_existing_subtitles: boolean;
			require_text_subtitle: boolean;
		};
		transcode: {
			require_manual_approval: boolean;
			allow_automatic_transcode: boolean;
		};
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
	let desiredState = $state<DesiredMediaState>(defaultDesiredMediaState());
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
		desiredState = normalizeDesiredMediaState(sidecar.state?.preferred_policy_state);
		loading = false;
	}

	async function runDryRun() {
		busy = true;
		error = '';
		const response = await apiFetch('/api/sidecar/dry-run', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ media_path: mediaPath, item_uid: itemUid, desired_state: desiredState })
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
			body: JSON.stringify({
				media_path: mediaPath,
				item_uid: itemUid,
				plan_hash: plan.plan_hash,
				desired_state: desiredState
			})
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

	function defaultDesiredMediaState(): DesiredMediaState {
		return {
			container: 'mkv',
			video: {
				preferred_codec: 'av1',
				minimum_allowed_codec: 'h264',
				allow_manual_codec_upgrade: false
			},
			audio: {
				allowed_layouts: ['stereo', 'surround51', 'surround71'],
				require_stereo_track: true,
				add_night_mode_stereo_track: false,
				transcode_stereo_to_opus: true,
				transcode_standard_surround_to_opus: true,
				preserve_object_audio_tracks: true,
				night_mode_target_lufs: -16
			},
			subtitles: {
				keep_existing_subtitles: true,
				require_text_subtitle: false
			},
			transcode: {
				require_manual_approval: true,
				allow_automatic_transcode: false
			}
		};
	}

	function normalizeDesiredMediaState(input: unknown): DesiredMediaState {
		const fallback = defaultDesiredMediaState();
		if (!input || typeof input !== 'object') {
			return fallback;
		}

		const value = input as Partial<DesiredMediaState>;
		const allowedLayouts = Array.isArray(value.audio?.allowed_layouts)
			? value.audio?.allowed_layouts.filter((layout): layout is 'stereo' | 'surround51' | 'surround71' =>
				layout === 'stereo' || layout === 'surround51' || layout === 'surround71'
			)
			: fallback.audio.allowed_layouts;

		return {
			container: value.container === 'mp4' ? 'mp4' : 'mkv',
			video: {
				preferred_codec:
					value.video?.preferred_codec === 'hevc' || value.video?.preferred_codec === 'h264'
						? value.video.preferred_codec
						: 'av1',
				minimum_allowed_codec: value.video?.minimum_allowed_codec === 'hevc' ? 'hevc' : 'h264',
				allow_manual_codec_upgrade: Boolean(value.video?.allow_manual_codec_upgrade)
			},
			audio: {
				allowed_layouts: allowedLayouts.length > 0 ? allowedLayouts : fallback.audio.allowed_layouts,
				require_stereo_track:
					value.audio?.require_stereo_track ?? fallback.audio.require_stereo_track,
				add_night_mode_stereo_track:
					value.audio?.add_night_mode_stereo_track ?? fallback.audio.add_night_mode_stereo_track,
				transcode_stereo_to_opus:
					value.audio?.transcode_stereo_to_opus ?? fallback.audio.transcode_stereo_to_opus,
				transcode_standard_surround_to_opus:
					value.audio?.transcode_standard_surround_to_opus ?? fallback.audio.transcode_standard_surround_to_opus,
				preserve_object_audio_tracks:
					value.audio?.preserve_object_audio_tracks ?? fallback.audio.preserve_object_audio_tracks,
				night_mode_target_lufs:
					typeof value.audio?.night_mode_target_lufs === 'number'
						? value.audio.night_mode_target_lufs
						: fallback.audio.night_mode_target_lufs
			},
			subtitles: {
				keep_existing_subtitles:
					value.subtitles?.keep_existing_subtitles ?? fallback.subtitles.keep_existing_subtitles,
				require_text_subtitle:
					value.subtitles?.require_text_subtitle ?? fallback.subtitles.require_text_subtitle
			},
			transcode: {
				require_manual_approval:
					value.transcode?.require_manual_approval ?? fallback.transcode.require_manual_approval,
				allow_automatic_transcode:
					value.transcode?.allow_automatic_transcode ?? fallback.transcode.allow_automatic_transcode
			}
		};
	}

	function toggleAudioLayout(layout: 'stereo' | 'surround51' | 'surround71') {
		const hasLayout = desiredState.audio.allowed_layouts.includes(layout);
		desiredState.audio.allowed_layouts = hasLayout
			? desiredState.audio.allowed_layouts.filter((value) => value !== layout)
			: [...desiredState.audio.allowed_layouts, layout];
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

		<div class="policy-grid">
			<label>
				<span>Container</span>
				<select bind:value={desiredState.container}>
					<option value="mkv">MKV (default)</option>
					<option value="mp4">MP4</option>
				</select>
			</label>

			<label>
				<span>Preferred Video Codec</span>
				<select bind:value={desiredState.video.preferred_codec}>
					<option value="av1">AV1 (preferred)</option>
					<option value="hevc">HEVC</option>
					<option value="h264">H.264</option>
				</select>
			</label>

			<label>
				<span>Minimum Allowed Video Codec</span>
				<select bind:value={desiredState.video.minimum_allowed_codec}>
					<option value="h264">H.264 (minimum)</option>
					<option value="hevc">HEVC</option>
				</select>
			</label>
		</div>

		<div class="checkbox-grid">
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.video.allow_manual_codec_upgrade} />
				<span>Allow manual codec upgrades (for example HEVC -&gt; AV1)</span>
			</label>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.transcode.require_manual_approval} />
				<span>Require manual approval before any transcode</span>
			</label>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.transcode.allow_automatic_transcode} />
				<span>Allow automatic transcode jobs</span>
			</label>
		</div>

		<div class="checkbox-grid">
			<p class="mono section-title">Allowed audio layouts</p>
			<label class="check">
				<input
					type="checkbox"
					checked={desiredState.audio.allowed_layouts.includes('stereo')}
					onchange={() => toggleAudioLayout('stereo')}
				/>
				<span>Stereo</span>
			</label>
			<label class="check">
				<input
					type="checkbox"
					checked={desiredState.audio.allowed_layouts.includes('surround51')}
					onchange={() => toggleAudioLayout('surround51')}
				/>
				<span>5.1 Surround</span>
			</label>
			<label class="check">
				<input
					type="checkbox"
					checked={desiredState.audio.allowed_layouts.includes('surround71')}
					onchange={() => toggleAudioLayout('surround71')}
				/>
				<span>7.1 Surround</span>
			</label>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.audio.require_stereo_track} />
				<span>Require at least one stereo track</span>
			</label>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.audio.add_night_mode_stereo_track} />
				<span>Add optional night-mode stereo track (loudness envelope)</span>
			</label>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.audio.transcode_stereo_to_opus} />
				<span>Default stereo transcode codec: Opus</span>
			</label>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.audio.transcode_standard_surround_to_opus} />
				<span>Default 5.1/7.1 transcode codec: Opus</span>
			</label>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.audio.preserve_object_audio_tracks} />
				<span>Preserve immersive/object audio (Atmos and similar) by default</span>
			</label>
			<label>
				<span>Night-mode target loudness (LUFS)</span>
				<input type="number" step="0.5" bind:value={desiredState.audio.night_mode_target_lufs} />
			</label>
		</div>

		<div class="checkbox-grid">
			<p class="mono section-title">Subtitle policy</p>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.subtitles.keep_existing_subtitles} />
				<span>Keep existing subtitle tracks</span>
			</label>
			<label class="check">
				<input type="checkbox" bind:checked={desiredState.subtitles.require_text_subtitle} />
				<span>Require at least one text subtitle track</span>
			</label>
		</div>

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
	select,
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

	.policy-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
		gap: 0.7rem;
		margin-top: 0.9rem;
	}

	.checkbox-grid {
		display: grid;
		gap: 0.35rem;
		margin-top: 0.9rem;
	}

	.check {
		display: flex;
		align-items: center;
		gap: 0.55rem;
		min-width: 100%;
	}

	.check input[type='checkbox'] {
		width: auto;
	}

	.section-title {
		margin: 0 0 0.2rem;
		color: var(--muted);
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
