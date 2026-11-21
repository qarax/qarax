<script lang="ts">
	import { resolve } from '$app/paths';
	import { allVms, hostsWithCapacity, type Host, type Vm } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { live } from '#lib/live.svelte.ts';
	import { HOST_STATUS, VM_STATUS, vmStatusLook, type StatusLook, type Tone } from '#lib/status.ts';
	import { bytes, clock } from '#lib/format.ts';
	import CapacityBar from '#lib/components/CapacityBar.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import StatusIcon from '#lib/components/StatusIcon.svelte';

	const overview = new Remote(async () => {
		const [units, vms] = await Promise.all([hostsWithCapacity(), allVms()]);
		return { units, vms };
	});
	keepFresh(overview.refresh);

	const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
	const sentence = (text: string) => (/[.!?]$/.test(text.trim()) ? text.trim() : `${text.trim()}.`);

	/** "web-1", "web-1 and web-2", "web-1, web-2 and 3 more". */
	function names(vms: Vm[]): string {
		const shown = vms.slice(0, 2).map((vm) => vm.name);
		const rest = vms.length - shown.length;
		return rest > 0 ? `${shown.join(', ')} and ${rest} more` : shown.join(' and ');
	}

	/** Something that needs a person: what is wrong, and what to do about it. */
	interface Attention {
		key: string;
		tone: 'danger' | 'warning';
		title: string;
		detail: string;
		href: string;
	}

	function needsAttention(hosts: Host[], vms: Vm[]): Attention[] {
		const items: Attention[] = [];
		for (const host of hosts) {
			const on = vms.filter((vm) => vm.host_id === host.id);
			const affected = on.length
				? `${on.length === 1 ? 'Its VM' : 'Its VMs'}, ${names(on)}, can't be managed until it's back.`
				: 'No VMs run on it.';
			const href = resolve('/hosts');
			switch (host.status) {
				case 'down':
					items.push({
						key: host.id,
						tone: 'danger',
						title: `${host.name} is down`,
						detail: `It isn't answering at ${host.address}:${host.port}. ${affected} Check that qarax-node is running on it.`,
						href
					});
					break;
				case 'installation_failed':
					items.push({
						key: host.id,
						tone: 'danger',
						title: `${host.name} failed to install`,
						detail: `${sentence(host.installation_error ?? 'Its last deploy or upgrade failed')} Fix the cause, then deploy it again with qarax host deploy.`,
						href
					});
					break;
				case 'unknown':
					items.push({
						key: host.id,
						tone: 'warning',
						title: `${host.name} reports no state`,
						detail: `qarax hasn't heard from it yet. ${affected}`,
						href
					});
					break;
				case 'up':
				case 'installing':
				case 'initializing':
				case 'maintenance':
					break;
			}
		}
		for (const vm of vms.filter((vm) => vm.status === 'unknown')) {
			items.push({
				key: vm.id,
				tone: 'warning',
				title: `${vm.name}'s state is unknown`,
				detail: "Its host isn't reporting it, so it may or may not be running.",
				href: resolve('/vms/[id]', { id: vm.id })
			});
		}
		return items;
	}

	/** How many have each status, in the order the statuses are listed, for an inventory card. */
	function tally<S extends string>(
		statuses: S[],
		looks: Record<S, StatusLook>
	): [Tone, string, number][] {
		return (Object.keys(looks) as S[])
			.map((status) => [status, statuses.filter((s) => s === status).length] as const)
			.filter(([, count]) => count > 0)
			.map(([status, count]) => [looks[status].tone, looks[status].text, count]);
	}
</script>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Overview</h1>
		<p class="pf-v6-c-content--p qx-subtle">What runs where, and whether anything needs you.</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={overview} what="the overview">
			{#snippet children({ units, vms })}
				{@const hosts = units.map(({ host }) => host)}
				{@const items = needsAttention(hosts, vms)}
				<div class="pf-v6-l-grid pf-m-gutter">
					<div class="pf-v6-l-grid__item pf-m-6-col-on-md pf-m-3-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">
									<a href={resolve('/hosts')}>{plural(hosts.length, 'Host', 'Hosts')}</a>
								</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if hosts.length}
									<ul class="tally" role="list">
										{#each tally( hosts.map((host) => host.status), HOST_STATUS ) as [tone, text, count] (text)}
											<li class="qx-status"><StatusIcon {tone} />{count} {text.toLowerCase()}</li>
										{/each}
									</ul>
								{:else}
									<span class="qx-subtle">None registered yet.</span>
								{/if}
							</div>
						</div>
					</div>
					<div class="pf-v6-l-grid__item pf-m-6-col-on-md pf-m-3-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">
									<a href={resolve('/vms')}
										>{plural(vms.length, 'Virtual machine', 'Virtual machines')}</a
									>
								</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if vms.length}
									<ul class="tally" role="list">
										{#each tally( vms.map((vm) => vm.status), VM_STATUS ) as [tone, text, count] (text)}
											<li class="qx-status"><StatusIcon {tone} />{count} {text.toLowerCase()}</li>
										{/each}
									</ul>
								{:else}
									<span class="qx-subtle">None created yet.</span>
								{/if}
							</div>
						</div>
					</div>
					<div class="pf-v6-l-grid__item pf-m-12-col-on-md pf-m-6-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Needs attention</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if items.length}
									<ul class="pf-v6-c-alert-group attention" role="list">
										{#each items as item (item.key)}
											<li class="pf-v6-c-alert-group__item">
												<Notice tone={item.tone} title={item.title} plain>
													{item.detail} <a href={item.href}>Details</a>
												</Notice>
											</li>
										{/each}
									</ul>
								{:else if hosts.length === 0}
									<p>
										Register a host to run VMs on: <code>qarax host add</code>, then
										<code>qarax host deploy</code>. docs/INSTALL.md walks through it.
									</p>
								{:else}
									<span class="qx-status"
										><StatusIcon tone="run" />Nothing needs your attention.</span
									>
								{/if}
							</div>
						</div>
					</div>

					{#if units.length}
						<div class="pf-v6-l-grid__item pf-m-12-col-on-md pf-m-8-col-on-xl">
							<div class="pf-v6-c-card pf-m-full-height">
								<div class="pf-v6-c-card__title">
									<h2 class="pf-v6-c-card__title-text">Host utilization</h2>
								</div>
								<div class="pf-v6-c-card__body">
									<table
										class="pf-v6-c-table pf-m-compact pf-m-grid-md"
										aria-label="Host utilization"
									>
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Host</th>
												<th class="pf-v6-c-table__th" scope="col">vCPUs allocated</th>
												<th class="pf-v6-c-table__th" scope="col">Memory allocated</th>
												<th class="pf-v6-c-table__th" scope="col">VMs</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each units as { host, capacity } (host.id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td" data-label="Host">
														<span class="qx-status"
															><StatusIcon
																tone={HOST_STATUS[host.status].tone}
																label={HOST_STATUS[host.status].text}
															/>{host.name}</span
														>
													</td>
													<td class="pf-v6-c-table__td" data-label="vCPUs allocated">
														<CapacityBar
															label="vCPUs"
															used={capacity.allocated_vcpus}
															total={capacity.total_cpus}
														/>
													</td>
													<td class="pf-v6-c-table__td" data-label="Memory allocated">
														<CapacityBar
															label="Memory"
															used={capacity.allocated_memory_bytes}
															total={capacity.total_memory_bytes}
															format={bytes}
														/>
													</td>
													<td class="pf-v6-c-table__td" data-label="VMs">
														{vms.filter((vm) => vm.host_id === host.id).length}
													</td>
												</tr>
											{/each}
										</tbody>
									</table>
								</div>
							</div>
						</div>
					{/if}

					<div class="pf-v6-l-grid__item pf-m-12-col-on-md pf-m-4-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Recent changes</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if live.recent.length === 0}
									<p class="qx-subtle">
										Nothing has changed since you opened this page. Starts, stops and other changes
										show up here as they happen.
									</p>
								{:else}
									<ul class="changes" role="list">
										{#each live.recent as change (change.seq)}
											{@const to = vmStatusLook(change.new_status)}
											<li>
												<StatusIcon tone={to.tone} />
												<span>
													<a href={resolve('/vms/[id]', { id: change.vm_id })}>{change.vm_name}</a>
													went from {vmStatusLook(change.previous_status).text.toLowerCase()} to
													{to.text.toLowerCase()}
												</span>
												<time class="qx-subtle" datetime={change.timestamp}
													>{clock(change.timestamp)}</time
												>
											</li>
										{/each}
									</ul>
								{/if}
							</div>
						</div>
					</div>
				</div>
			{/snippet}
		</Loaded>
	</div>
</section>

<style>
	.tally,
	.changes,
	.attention {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--pf-t--global--spacer--sm);
	}

	.tally {
		font-size: var(--pf-t--global--font--size--body--lg);
	}

	.changes li {
		display: grid;
		grid-template-columns: auto 1fr auto;
		gap: var(--pf-t--global--spacer--sm);
		align-items: baseline;
	}

	time {
		font-size: var(--pf-t--global--font--size--body--sm);
		font-variant-numeric: tabular-nums;
	}

	td :global(.pf-v6-c-progress) {
		min-width: 10rem;
	}
</style>
