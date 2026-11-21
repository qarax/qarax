<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import Empty from '#lib/components/Empty.svelte';
</script>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Empty
			icon={page.status === 404 ? 'unknown' : 'danger'}
			title={page.status === 404 ? "There's no page here" : "This page didn't load"}
		>
			{page.status === 404
				? 'The address may be mistyped, or the page may have moved.'
				: page.error?.message}
			{#snippet footer()}
				<div class="pf-v6-c-empty-state__actions">
					<a class="pf-v6-c-button pf-m-primary" href={resolve('/')}>
						<span class="pf-v6-c-button__text">Go to the overview</span>
					</a>
				</div>
			{/snippet}
		</Empty>
	</div>
</section>
