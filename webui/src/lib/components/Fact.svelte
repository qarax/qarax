<script lang="ts">
	import type { Snippet } from 'svelte';

	/** One term and its value in a PatternFly description list. */
	let { term, children }: { term: string; children: Snippet } = $props();
</script>

<div class="pf-v6-c-description-list__group">
	<dt class="pf-v6-c-description-list__term">
		<span class="pf-v6-c-description-list__text">{term}</span>
	</dt>
	<dd class="pf-v6-c-description-list__description">
		<div class="pf-v6-c-description-list__text">{@render children()}</div>
	</dd>
</div>
