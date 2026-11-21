<script lang="ts">
	import type { StatusLook, Tone } from '#lib/status.ts';
	import Icon from './Icon.svelte';
	import { TONE_ICON } from './StatusIcon.svelte';

	/** A status as a PatternFly label: green running, red failed, blue in progress, grey otherwise. */
	let { look }: { look: StatusLook } = $props();

	const COLOR: Partial<Record<Tone, string>> = {
		run: 'pf-m-success',
		fault: 'pf-m-danger',
		busy: 'pf-m-info'
	};
</script>

<span class="pf-v6-c-label {COLOR[look.tone] ?? ''}">
	<span class="pf-v6-c-label__content">
		<span class="pf-v6-c-label__icon"><Icon name={TONE_ICON[look.tone]} /></span>
		<span class="pf-v6-c-label__text">{look.text}</span>
	</span>
</span>
