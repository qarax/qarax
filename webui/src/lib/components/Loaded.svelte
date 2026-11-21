<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';
	import type { Remote } from '#lib/remote.svelte.ts';
	import Notice from './Notice.svelte';

	/** Renders a {@link Remote}'s data once it loads, and says when loading it failed. */
	let { remote, what, children }: { remote: Remote<T>; what: string; children: Snippet<[T]> } =
		$props();
</script>

{#if remote.state.kind === 'loading'}
	<div class="loading">
		<svg
			class="pf-v6-c-spinner pf-m-lg"
			role="progressbar"
			viewBox="0 0 100 100"
			aria-label="Loading {what}"
		>
			<circle class="pf-v6-c-spinner__path" cx="50" cy="50" r="45" fill="none" />
		</svg>
	</div>
{:else if remote.state.kind === 'failed'}
	<Notice tone="danger" title="Couldn't load {what}">
		{remote.state.error}
		{#snippet actions()}
			<button class="pf-v6-c-button pf-m-link pf-m-inline" type="button" onclick={remote.refresh}>
				<span class="pf-v6-c-button__text">Try again</span>
			</button>
		{/snippet}
	</Notice>
{:else}
	{#if remote.state.kind === 'stale'}
		<div class="stale">
			<Notice tone="warning" title="Couldn't refresh {what}">
				{remote.state.error} What you see is from the last load.
			</Notice>
		</div>
	{/if}
	{@render children(remote.state.data)}
{/if}

<style>
	.loading {
		display: grid;
		place-items: center;
		padding: var(--pf-t--global--spacer--3xl) 0;
	}

	.stale {
		margin-bottom: var(--pf-t--global--spacer--md);
	}
</style>
