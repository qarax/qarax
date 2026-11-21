<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from './Icon.svelte';

	/**
	 * A PatternFly form group: a label, its control (`children`), and its error
	 * and helper text. The label is for the control with the group's `id`; a
	 * group of radios or checkboxes (`choices`) has no single control, so its
	 * label names the group instead. Controls describe themselves with
	 * `{id}-helper` when there is an error or helper text.
	 */
	let {
		id,
		label,
		required = false,
		choices,
		error,
		helper,
		children
	}: {
		id: string;
		label: string;
		required?: boolean;
		choices?: 'radio' | 'checkbox';
		error?: string;
		helper?: Snippet;
		children: Snippet;
	} = $props();
</script>

<div
	class="pf-v6-c-form__group"
	role={choices === 'radio' ? 'radiogroup' : choices ? 'group' : undefined}
	aria-labelledby={choices ? `${id}-label` : undefined}
>
	<div class="pf-v6-c-form__group-label" class:pf-m-no-padding-top={choices} id="{id}-label">
		{#if choices}
			<span class="pf-v6-c-form__label">
				<span class="pf-v6-c-form__label-text">{label}</span>
			</span>
		{:else}
			<label class="pf-v6-c-form__label" for={id}>
				<span class="pf-v6-c-form__label-text">{label}</span>{#if required}&nbsp;<span
						class="pf-v6-c-form__label-required"
						aria-hidden="true">&#42;</span
					>{/if}
			</label>
		{/if}
	</div>
	<div class="pf-v6-c-form__group-control" class:pf-m-stack={choices}>
		{@render children()}
		{#if error || helper}
			<div class="pf-v6-c-form__helper-text" aria-live="polite">
				<div class="pf-v6-c-helper-text" id="{id}-helper">
					{#if error}
						<div class="pf-v6-c-helper-text__item pf-m-error">
							<span class="pf-v6-c-helper-text__item-icon"><Icon name="danger" /></span>
							<span class="pf-v6-c-helper-text__item-text">
								<span class="pf-v6-screen-reader">Error:</span>
								{error}
							</span>
						</div>
					{/if}
					{#if helper}
						<div class="pf-v6-c-helper-text__item">
							<span class="pf-v6-c-helper-text__item-text">{@render helper()}</span>
						</div>
					{/if}
				</div>
			</div>
		{/if}
	</div>
</div>
