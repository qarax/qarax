<script lang="ts">
	import { resolve } from '$app/paths';
	import { allNetworks } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { NETWORK_STATUS } from '#lib/status.ts';
	import CodeBlock from '#lib/components/CodeBlock.svelte';
	import Empty from '#lib/components/Empty.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Status from '#lib/components/Status.svelte';

	const list = new Remote(allNetworks);
	keepFresh(list.refresh);
</script>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Networks</h1>
		<p class="pf-v6-c-content--p qx-subtle">
			The subnets qarax hands VMs addresses on, and the hosts that carry them.
		</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={list} what="networks">
			{#snippet children(networks)}
				<div>
					{#if networks.length === 0}
						<Empty icon="network" title="No networks yet">
							A network is a subnet qarax manages: it gives each VM on it an address, and a bridge
							on each host it is attached to carries its traffic. Create one and attach it to a host
							from the CLI:
							<CodeBlock
								code={'qarax network create --name lan --subnet 10.0.0.0/24 --gateway 10.0.0.1\nqarax network attach-host --network lan --host node-01 --bridge-name qbr0'}
							/>
						</Empty>
					{:else}
						<table class="pf-v6-c-table pf-m-grid-md" aria-label="Networks">
							<thead class="pf-v6-c-table__thead">
								<tr class="pf-v6-c-table__tr">
									<th class="pf-v6-c-table__th" scope="col">Name</th>
									<th class="pf-v6-c-table__th" scope="col">Status</th>
									<th class="pf-v6-c-table__th" scope="col">Subnet</th>
									<th class="pf-v6-c-table__th" scope="col">Gateway</th>
									<th class="pf-v6-c-table__th" scope="col">Type</th>
									<th class="pf-v6-c-table__th" scope="col">VPC</th>
								</tr>
							</thead>
							<tbody class="pf-v6-c-table__tbody">
								{#each networks as network (network.id)}
									<tr class="pf-v6-c-table__tr">
										<td class="pf-v6-c-table__td" data-label="Name">
											<a href={resolve('/networks/[id]', { id: network.id })}>{network.name}</a>
										</td>
										<td class="pf-v6-c-table__td" data-label="Status"
											><Status look={NETWORK_STATUS[network.status]} /></td
										>
										<td class="pf-v6-c-table__td qx-mono" data-label="Subnet">{network.subnet}</td>
										<td class="pf-v6-c-table__td" data-label="Gateway">
											{#if network.gateway}<span class="qx-mono">{network.gateway}</span
												>{:else}<span class="qx-subtle">None</span>{/if}
										</td>
										<td class="pf-v6-c-table__td" data-label="Type">
											{#if network.type}{network.type}{:else}<span class="qx-subtle">Not set</span
												>{/if}
										</td>
										<td class="pf-v6-c-table__td" data-label="VPC">
											{#if network.vpc_name}{network.vpc_name}{:else}<span class="qx-subtle"
													>None</span
												>{/if}
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{/if}
				</div>
			{/snippet}
		</Loaded>
	</div>
</section>
