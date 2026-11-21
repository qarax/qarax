<script lang="ts">
	/** Commands to type, in a PatternFly code block. */
	let { code }: { code: string } = $props();
</script>

<div class="pf-v6-c-code-block">
	<div class="pf-v6-c-code-block__content">
		<pre class="pf-v6-c-code-block__pre"><code class="pf-v6-c-code-block__code">{code}</code></pre>
	</div>
</div>

<style>
	.pf-v6-c-code-block {
		text-align: start;
		margin-block: var(--pf-t--global--spacer--md);
	}
</style>
