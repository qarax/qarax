<script lang="ts">
	import { onMount, type Snippet } from 'svelte';
	import Icon from './Icon.svelte';

	/**
	 * A PatternFly modal that asks before an action that is hard to take back,
	 * open while it is mounted: render it while asking. A native <dialog> brings
	 * focus handling and Escape; Escape, Close and Cancel call `oncancel`.
	 */
	let {
		title,
		confirm,
		tone = 'danger',
		onconfirm,
		oncancel,
		children
	}: {
		title: string;
		confirm: string;
		tone?: 'danger' | 'warning';
		onconfirm: () => void;
		oncancel: () => void;
		children: Snippet;
	} = $props();

	const id = $props.id();
	let dialog: HTMLDialogElement;
	onMount(() => dialog.showModal());

	/** Escape: the owner decides, by unmounting this. */
	function escape(event: Event) {
		event.preventDefault();
		oncancel();
	}
</script>

<dialog class="qx-modal" bind:this={dialog} oncancel={escape} aria-labelledby="{id}-title">
	<div class="pf-v6-c-modal-box pf-m-sm pf-m-{tone}">
		<div class="pf-v6-c-modal-box__close">
			<button class="pf-v6-c-button pf-m-plain" type="button" aria-label="Close" onclick={oncancel}>
				<span class="pf-v6-c-button__icon"><Icon name="close" /></span>
			</button>
		</div>
		<header class="pf-v6-c-modal-box__header">
			<div class="pf-v6-c-modal-box__header-main">
				<h1 class="pf-v6-c-modal-box__title pf-m-icon pf-m-{tone}" id="{id}-title">
					<span class="pf-v6-c-modal-box__title-icon"><Icon name={tone} /></span>
					<span class="pf-v6-c-modal-box__title-text">{title}</span>
				</h1>
			</div>
		</header>
		<div class="pf-v6-c-modal-box__body">{@render children()}</div>
		<footer class="pf-v6-c-modal-box__footer">
			<button
				class="pf-v6-c-button {tone === 'danger' ? 'pf-m-danger' : 'pf-m-primary'}"
				type="button"
				onclick={onconfirm}
			>
				<span class="pf-v6-c-button__text">{confirm}</span>
			</button>
			<button class="pf-v6-c-button pf-m-link" type="button" onclick={oncancel}>
				<span class="pf-v6-c-button__text">Cancel</span>
			</button>
		</footer>
	</div>
</dialog>
