<script lang="ts">
	import type { PageProps } from './$types';
	import PoolView from './PoolView.svelte';

	let { params }: PageProps = $props();
</script>

{#key params.id}
	<PoolView id={params.id} />
{/key}
