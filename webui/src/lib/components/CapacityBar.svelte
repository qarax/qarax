<script lang="ts">
	/**
	 * How much of a host's capacity is taken, as a small PatternFly progress
	 * bar for a table cell: the numbers above the bar. Warning from 80 %,
	 * danger past 100 % (oversubscribed, which the scheduler allows for CPUs).
	 */
	let {
		label,
		used,
		total,
		format = String
	}: {
		label: string;
		used: number;
		total: number | null | undefined;
		format?: (n: number) => string;
	} = $props();

	const ratio = $derived(total ? used / total : 0);
	const level = $derived(ratio > 1 ? 'pf-m-danger' : ratio >= 0.8 ? 'pf-m-warning' : '');
	const measure = $derived(total ? `${format(used)} of ${format(total)}` : 'Not reported yet');
</script>

{#if total}
	<div class="pf-v6-c-progress pf-m-sm {level}">
		<div class="pf-v6-c-progress__description">{measure}</div>
		<div
			class="pf-v6-c-progress__bar"
			role="progressbar"
			aria-valuemin={0}
			aria-valuemax={100}
			aria-valuenow={Math.round(Math.min(ratio, 1) * 100)}
			aria-valuetext={measure}
			aria-label={label}
		>
			<div class="pf-v6-c-progress__indicator" style:width="{Math.min(100, ratio * 100)}%"></div>
		</div>
	</div>
{:else}
	<span class="qx-subtle">{measure}</span>
{/if}

<style>
	.pf-v6-c-progress {
		min-width: 8rem;
	}

	.pf-v6-c-progress__description {
		white-space: nowrap;
	}
</style>
