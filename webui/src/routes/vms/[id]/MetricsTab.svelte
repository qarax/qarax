<script lang="ts">
	import type { Vm } from '#lib/api/client.ts';
	import { DEVICE_KINDS, deviceRates, type MetricsSampler } from '#lib/metrics.svelte.ts';
	import { bytes } from '#lib/format.ts';
	import Notice from '#lib/components/Notice.svelte';
	import RateChart from '#lib/components/RateChart.svelte';

	/** A VM's disk and network rates, charted from what `sampler` reads while this tab is open. */
	let { vm, sampler }: { vm: Vm; sampler: MetricsSampler } = $props();

	const devices = $derived(deviceRates(sampler.samples));
	const end = $derived(sampler.samples.at(-1)?.at ?? Date.now());
	const rate = (value: number) => `${bytes(value)}/s`;
</script>

<p class="qx-subtle intro">
	How fast {vm.name}'s disks and network interfaces move data, read every 2 seconds while this tab
	is open. qarax keeps no history of it, so the charts start when you open the tab. The charts and
	the peaks cover the last 5 minutes.
</p>

{#if vm.status !== 'running'}
	<p class="qx-subtle">The charts run while {vm.name} runs.</p>
{:else if sampler.error && sampler.samples.length === 0}
	<Notice tone="danger" title="Couldn't read its counters">{sampler.error}</Notice>
{:else if sampler.samples.length === 0}
	<p class="qx-subtle">Reading its counters…</p>
{:else if devices.length === 0}
	<p class="qx-subtle">
		{#if vm.hypervisor === 'firecracker'}
			qarax doesn't read device counters from Firecracker VMs yet, so there is nothing to chart for
			{vm.name}.
		{:else}
			None of {vm.name}'s devices reports counters right now.
		{/if}
	</p>
{:else}
	{#if sampler.error}
		<div class="stale">
			<Notice tone="warning" title="Couldn't read its counters">
				{sampler.error} The charts pick up again with the next reading that works.
			</Notice>
		</div>
	{/if}
	<div class="pf-v6-l-grid pf-m-gutter">
		{#each devices as { device, kind, points, totals } (device)}
			{@const series = DEVICE_KINDS[kind].series}
			{@const readings = points.filter((point) => point !== null)}
			<div class="pf-v6-l-grid__item pf-m-12-col pf-m-6-col-on-xl">
				<div class="pf-v6-c-card pf-m-full-height">
					<div class="pf-v6-c-card__title">
						<h2 class="pf-v6-c-card__title-text">
							{DEVICE_KINDS[kind].title} <span class="qx-mono">{device}</span>
						</h2>
					</div>
					<div class="pf-v6-c-card__body">
						<RateChart label="{DEVICE_KINDS[kind].title} {device}" {series} {points} {end} />
						<table
							class="pf-v6-c-table pf-m-compact figures"
							aria-label="{DEVICE_KINDS[kind].title} {device} figures"
						>
							<thead class="pf-v6-c-table__thead">
								<tr class="pf-v6-c-table__tr">
									<th class="pf-v6-c-table__th" scope="col"></th>
									<th class="pf-v6-c-table__th" scope="col">Now</th>
									<th class="pf-v6-c-table__th" scope="col">Peak</th>
									<th class="pf-v6-c-table__th" scope="col">Since it booted</th>
								</tr>
							</thead>
							<tbody class="pf-v6-c-table__tbody">
								{#each series as s, i (s.key)}
									<tr class="pf-v6-c-table__tr">
										<th class="pf-v6-c-table__th" scope="row">{s.label}</th>
										<td class="pf-v6-c-table__td" data-label="Now">
											{readings.length ? rate(readings[readings.length - 1].values[i]) : '–'}
										</td>
										<td class="pf-v6-c-table__td" data-label="Peak">
											{readings.length
												? rate(Math.max(...readings.map((point) => point.values[i])))
												: '–'}
										</td>
										<td class="pf-v6-c-table__td" data-label="Since it booted"
											>{bytes(totals[i])}</td
										>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</div>
			</div>
		{/each}
	</div>
{/if}

<style>
	.intro,
	.stale {
		margin-bottom: var(--pf-t--global--spacer--md);
	}

	.figures {
		margin-top: var(--pf-t--global--spacer--md);
		font-variant-numeric: tabular-nums;
	}
</style>
