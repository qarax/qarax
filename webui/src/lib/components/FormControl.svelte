<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from './Icon.svelte';

	/**
	 * PatternFly's frame for an input, select or textarea (`children`): its
	 * look when invalid, with the status icon, and a select's toggle icon.
	 */
	let {
		kind = 'input',
		invalid = false,
		children
	}: { kind?: 'input' | 'select' | 'textarea'; invalid?: boolean; children: Snippet } = $props();
</script>

<span
	class="pf-v6-c-form-control"
	class:pf-m-textarea={kind === 'textarea'}
	class:pf-m-error={invalid}
>
	{@render children()}
	{#if invalid || kind === 'select'}
		<span class="pf-v6-c-form-control__utilities">
			{#if invalid}
				<span class="pf-v6-c-form-control__icon pf-m-status"><Icon name="danger" /></span>
			{/if}
			{#if kind === 'select'}
				<span class="pf-v6-c-form-control__toggle-icon"><Icon name="caretDown" /></span>
			{/if}
		</span>
	{/if}
</span>
