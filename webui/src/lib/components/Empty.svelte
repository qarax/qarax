<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon, { type IconName } from './Icon.svelte';

	/** A PatternFly empty state: what belongs here, and how to get it there. */
	let {
		icon,
		title,
		children,
		footer
	}: { icon: IconName; title: string; children: Snippet; footer?: Snippet } = $props();
</script>

<div class="pf-v6-c-empty-state">
	<div class="pf-v6-c-empty-state__content">
		<div class="pf-v6-c-empty-state__header">
			<div class="pf-v6-c-empty-state__icon"><Icon name={icon} /></div>
			<div class="pf-v6-c-empty-state__title">
				<h2 class="pf-v6-c-empty-state__title-text">{title}</h2>
			</div>
		</div>
		<div class="pf-v6-c-empty-state__body">{@render children()}</div>
		{#if footer}
			<div class="pf-v6-c-empty-state__footer">{@render footer()}</div>
		{/if}
	</div>
</div>
