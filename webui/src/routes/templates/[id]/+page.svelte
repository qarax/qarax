<script lang="ts">
	import type { PageProps } from './$types';
	import TemplateView from './TemplateView.svelte';

	let { params }: PageProps = $props();
</script>

<!-- Another template is another view: its own data and actions. -->
{#key params.id}
	<TemplateView id={params.id} />
{/key}
