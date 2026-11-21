<script lang="ts">
	import type { PageProps } from './$types';
	import VmView from './VmView.svelte';

	let { params }: PageProps = $props();
</script>

<!-- A new VM is a new view: its own data, actions and console. -->
{#key params.id}
	<VmView id={params.id} />
{/key}
