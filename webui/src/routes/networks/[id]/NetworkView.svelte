<script lang="ts">
	import { untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import { allVms, api, call } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { NETWORK_STATUS } from '#lib/status.ts';
	import { ago } from '#lib/format.ts';
	import Fact from '#lib/components/Fact.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import StatusLabel from '#lib/components/StatusLabel.svelte';

	let { id }: { id: string } = $props();

	// The page re-creates this view for another network ({#key}), so the id is fixed.
	const networkId = untrack(() => id);
	const path = { params: { path: { network_id: networkId } } };

	const detail = new Remote(async () => {
		const [network, hosts, addresses, vms] = await Promise.all([
			call(api.GET('/v1/networks/{network_id}', path)),
			call(api.GET('/v1/networks/{network_id}/hosts', path)),
			call(api.GET('/v1/networks/{network_id}/ips', path)),
			allVms()
		]);
		return { network, hosts, addresses, vmNames: new Map(vms.map((vm) => [vm.id, vm.name])) };
	});
	// Addresses come and go with VMs; the event stream says when VMs change.
	keepFresh(detail.refresh);
</script>

<section class="pf-v6-c-page__main-breadcrumb">
	<div class="pf-v6-c-page__main-body">
		<nav class="pf-v6-c-breadcrumb" aria-label="Breadcrumb">
			<ol class="pf-v6-c-breadcrumb__list" role="list">
				<li class="pf-v6-c-breadcrumb__item">
					<a href={resolve('/networks')} class="pf-v6-c-breadcrumb__link">Networks</a>
				</li>
				<li class="pf-v6-c-breadcrumb__item">
					<span class="pf-v6-c-breadcrumb__item-divider"><Icon name="chevronRight" /></span>
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page">
						{detail.state.kind === 'ready' || detail.state.kind === 'stale'
							? detail.state.data.network.name
							: 'Network'}
					</span>
				</li>
			</ol>
		</nav>
	</div>
</section>

<Loaded remote={detail} what="this network">
	{#snippet children({ network, hosts, addresses, vmNames })}
		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="title">
					<h1 class="pf-v6-c-title pf-m-2xl">{network.name}</h1>
					<StatusLabel look={NETWORK_STATUS[network.status]} />
				</div>
				<p class="pf-v6-c-content--p qx-subtle">
					VMs on it get an address in {network.subnet}{#if network.vpc_name}, and can reach the
						other networks of the VPC {network.vpc_name}{/if}.
				</p>
			</div>
		</section>

		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="pf-v6-l-grid pf-m-gutter">
					<div class="pf-v6-l-grid__item pf-m-12-col pf-m-6-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Details</h2>
							</div>
							<div class="pf-v6-c-card__body">
								<dl class="pf-v6-c-description-list pf-m-horizontal">
									<Fact term="Subnet"><span class="qx-mono">{network.subnet}</span></Fact>
									<Fact term="Gateway">
										{#if network.gateway}<span class="qx-mono">{network.gateway}</span>{:else}<span
												class="qx-subtle">None</span
											>{/if}
									</Fact>
									<Fact term="DNS server">
										{#if network.dns}<span class="qx-mono">{network.dns}</span>{:else}<span
												class="qx-subtle">None</span
											>{/if}
									</Fact>
									<Fact term="Type">
										{#if network.type}{network.type}{:else}<span class="qx-subtle">Not set</span
											>{/if}
									</Fact>
									<Fact term="VPC">
										{#if network.vpc_name}{network.vpc_name}{:else}<span class="qx-subtle"
												>None</span
											>{/if}
									</Fact>
									<Fact term="ID"><span class="qx-mono">{network.id}</span></Fact>
								</dl>
							</div>
						</div>
					</div>
					<div class="pf-v6-l-grid__item pf-m-12-col pf-m-6-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Hosts</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if hosts.length}
									<p class="qx-subtle intro">
										VMs on these hosts can join it, through the bridge on each.
									</p>
									<table class="pf-v6-c-table pf-m-compact" aria-label="Hosts">
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Host</th>
												<th class="pf-v6-c-table__th" scope="col">Bridge</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each hosts as host (host.host_id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td" data-label="Host">{host.host_name}</td>
													<td class="pf-v6-c-table__td qx-mono" data-label="Bridge"
														>{host.bridge_name}</td
													>
												</tr>
											{/each}
										</tbody>
									</table>
								{:else}
									<p class="qx-subtle">
										No host carries it yet, so no VM can join it. Attach one with <code
											>qarax network attach-host --network {network.name} --host &lt;host&gt; --bridge-name
											&lt;bridge&gt;</code
										>.
									</p>
								{/if}
							</div>
						</div>
					</div>

					<div class="pf-v6-l-grid__item pf-m-12-col">
						<div class="pf-v6-c-card">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Addresses in use</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if addresses.length === 0}
									<p class="qx-subtle">
										No VM has an address on it yet. A VM created on this network gets one.
									</p>
								{:else}
									<table class="pf-v6-c-table pf-m-compact pf-m-grid-md" aria-label="Addresses">
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Address</th>
												<th class="pf-v6-c-table__th" scope="col">VM</th>
												<th class="pf-v6-c-table__th" scope="col">Given</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each addresses as address (address.id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td qx-mono" data-label="Address"
														>{address.ip_address}</td
													>
													<td class="pf-v6-c-table__td" data-label="VM">
														{#if address.vm_id}
															<a href={resolve('/vms/[id]', { id: address.vm_id })}
																>{vmNames.get(address.vm_id) ?? address.vm_id}</a
															>
														{:else}
															<span class="qx-subtle">Reserved, not on a VM</span>
														{/if}
													</td>
													<td class="pf-v6-c-table__td" data-label="Given">
														<time datetime={address.allocated_at}>{ago(address.allocated_at)}</time>
													</td>
												</tr>
											{/each}
										</tbody>
									</table>
								{/if}
							</div>
						</div>
					</div>
				</div>
			</div>
		</section>
	{/snippet}
</Loaded>

<style>
	.title {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--pf-t--global--spacer--md);
		margin-bottom: var(--pf-t--global--spacer--sm);
	}

	.intro {
		margin-bottom: var(--pf-t--global--spacer--sm);
	}
</style>
