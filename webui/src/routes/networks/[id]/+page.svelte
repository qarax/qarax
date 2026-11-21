<script lang="ts">
	import type { PageProps } from './$types';
	import NetworkView from './NetworkView.svelte';

	let { params }: PageProps = $props();
</script>

{#key params.id}
	<NetworkView id={params.id} />
{/key}
