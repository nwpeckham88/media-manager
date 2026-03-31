<script lang="ts">
	type ConfirmTone = 'default' | 'danger';

	type Props = {
		open: boolean;
		title: string;
		message: string;
		confirmLabel?: string;
		cancelLabel?: string;
		busy?: boolean;
		tone?: ConfirmTone;
		onConfirm?: () => void | Promise<void>;
		onCancel?: () => void | Promise<void>;
	};

	let {
		open = false,
		title,
		message,
		confirmLabel = 'Confirm',
		cancelLabel = 'Cancel',
		busy = false,
		tone = 'default',
		onConfirm,
		onCancel
	}: Props = $props();

	function closeIfAllowed() {
		if (!busy) {
			onCancel?.();
		}
	}

	function handleBackdropClick(event: MouseEvent) {
		if (event.currentTarget === event.target) {
			closeIfAllowed();
		}
	}
</script>

<svelte:window
	onkeydown={(event) => {
		if (open && event.key === 'Escape') {
			closeIfAllowed();
		}
	}}
/>

{#if open}
	<div class="confirm-backdrop" role="presentation" onclick={handleBackdropClick}>
		<div class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-message">
			<h2 id="confirm-title">{title}</h2>
			<p id="confirm-message">{message}</p>
			<div class="actions">
				<button type="button" class="secondary" onclick={closeIfAllowed} disabled={busy}>{cancelLabel}</button>
				<button type="button" class={`primary ${tone}`} onclick={() => onConfirm?.()} disabled={busy}>{confirmLabel}</button>
			</div>
		</div>
	</div>
{/if}

<style>
	.confirm-backdrop {
		position: fixed;
		inset: 0;
		z-index: 30;
		display: grid;
		place-items: center;
		padding: var(--space-4);
		background: color-mix(in srgb, #0b1218 42%, transparent);
		backdrop-filter: blur(3px);
	}

	.confirm-dialog {
		width: min(520px, 94vw);
		display: grid;
		gap: var(--space-3);
		padding: var(--space-4);
		border-radius: var(--radius-lg);
		border: 1px solid var(--ring);
		background: color-mix(in srgb, var(--card) 96%, transparent);
		box-shadow: 0 10px 28px rgba(0, 0, 0, 0.18);
	}

	h2 {
		margin: 0;
		font-size: var(--font-body);
	}

	p {
		margin: 0;
		color: var(--muted);
		line-height: 1.45;
	}

	.actions {
		display: flex;
		justify-content: flex-end;
		gap: var(--space-2);
	}

	button {
		border-radius: var(--radius-md);
		border: 1px solid var(--ring);
		padding: 0.5rem 0.72rem;
		font: inherit;
		font-weight: 700;
		font-size: var(--font-small);
		cursor: pointer;
	}

	button:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.secondary {
		background: color-mix(in srgb, var(--card) 90%, transparent);
		color: var(--text);
	}

	.primary {
		background: color-mix(in srgb, var(--accent) 28%, var(--card));
		color: var(--text);
	}

	.primary.danger {
		background: color-mix(in srgb, var(--danger) 28%, var(--card));
	}
</style>
