<script lang="ts">
	import type { PageProps } from './$types';
	import HostView from './HostView.svelte';

	let { params }: PageProps = $props();
</script>

<!-- Another host is another view: its own data and actions. -->
{#key params.id}
	<HostView id={params.id} />
{/key}
