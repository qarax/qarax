<script lang="ts">
	import type { StatusLook } from '#lib/status.ts';
	import StatusIcon from './StatusIcon.svelte';

	let { look }: { look: StatusLook } = $props();
</script>

<span class="qx-status"><StatusIcon tone={look.tone} />{look.text}</span>
