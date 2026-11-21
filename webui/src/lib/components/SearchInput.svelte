<script lang="ts">
	import Icon from './Icon.svelte';

	/** A PatternFly search input. */
	let { value = $bindable(), label }: { value: string; label: string } = $props();
</script>

<div class="pf-v6-c-text-input-group">
	<div class="pf-v6-c-text-input-group__main pf-m-icon">
		<span class="pf-v6-c-text-input-group__text">
			<span class="pf-v6-c-text-input-group__icon"><Icon name="search" /></span>
			<input
				class="pf-v6-c-text-input-group__text-input"
				type="search"
				placeholder={label}
				aria-label={label}
				bind:value
			/>
		</span>
	</div>
</div>
