<script lang="ts" module>
	import type { Tone } from '#lib/status.ts';
	import type { IconName } from './Icon.svelte';

	/** PatternFly's status icons, as its products use them for VMs and hosts. */
	export const TONE_ICON: Record<Tone, IconName> = {
		run: 'ok',
		hold: 'paused',
		idle: 'off',
		busy: 'inProgress',
		fault: 'error',
		unknown: 'unknown'
	};
</script>

<script lang="ts">
	import Icon from './Icon.svelte';

	/** A status as an icon; `label` names it when no word is next to it. */
	let { tone, label }: { tone: Tone; label?: string } = $props();

	const COLOR: Partial<Record<Tone, string>> = { run: 'pf-m-success', fault: 'pf-m-danger' };
</script>

<span
	class="pf-v6-c-icon pf-m-inline"
	role={label ? 'img' : undefined}
	aria-label={label}
	aria-hidden={label ? undefined : true}
>
	<span class="pf-v6-c-icon__content {COLOR[tone] ?? ''}"><Icon name={TONE_ICON[tone]} /></span>
</span>
