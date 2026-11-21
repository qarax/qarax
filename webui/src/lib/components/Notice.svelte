<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from './Icon.svelte';

	/** A PatternFly inline alert: what happened, then what to do (`children`, `actions`). */
	let {
		tone,
		title,
		plain = false,
		children,
		actions
	}: {
		tone: 'danger' | 'warning';
		title: string;
		plain?: boolean;
		children?: Snippet;
		actions?: Snippet;
	} = $props();
</script>

<div
	class="pf-v6-c-alert pf-m-inline pf-m-{tone}"
	class:pf-m-plain={plain}
	role={tone === 'danger' ? 'alert' : 'status'}
>
	<div class="pf-v6-c-alert__icon"><Icon name={tone} /></div>
	<p class="pf-v6-c-alert__title">
		<span class="pf-v6-screen-reader">{tone === 'danger' ? 'Error' : 'Warning'}:</span>
		{title}
	</p>
	{#if children}
		<div class="pf-v6-c-alert__description">{@render children()}</div>
	{/if}
	{#if actions}
		<div class="pf-v6-c-alert__action-group">{@render actions()}</div>
	{/if}
</div>
