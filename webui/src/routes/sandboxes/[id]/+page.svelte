<script lang="ts">
	import type { PageProps } from './$types';
	import SandboxView from './SandboxView.svelte';

	let { params }: PageProps = $props();
</script>

<!-- Another sandbox is another view: its own data, actions and commands. -->
{#key params.id}
	<SandboxView id={params.id} />
{/key}
