<script lang="ts">
	import { resolve } from '$app/paths';
	import {
		allBootSources,
		allNetworks,
		allSandboxPools,
		allStorageObjects,
		allVmTemplates
	} from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { HYPERVISOR, capitalize } from '#lib/format.ts';
	import { templateBoots, templateSize } from '#lib/templates.ts';
	import CodeBlock from '#lib/components/CodeBlock.svelte';
	import Empty from '#lib/components/Empty.svelte';
	import Loaded from '#lib/components/Loaded.svelte';

	const list = new Remote(async () => {
		const [templates, bootSources, disks, networks, pools] = await Promise.all([
			allVmTemplates(),
			allBootSources(),
			allStorageObjects({ objectType: 'disk' }),
			allNetworks(),
			allSandboxPools()
		]);
		return {
			templates,
			names: {
				bootSources: new Map(bootSources.map((source) => [source.id, source.name])),
				disks: new Map(disks.map((disk) => [disk.id, disk.name]))
			},
			networkNames: new Map(networks.map((network) => [network.id, network.name])),
			pooled: new Set(pools.map((pool) => pool.vm_template_id))
		};
	});
	keepFresh(list.refresh);
</script>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">VM templates</h1>
		<p class="pf-v6-c-content--p qx-subtle">
			What a VM boots, its size and its network, kept under a name, so VMs and sandboxes of a kind
			are alike.
		</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={list} what="VM templates">
			{#snippet children({ templates, names, networkNames, pooled })}
				<div>
					{#if templates.length === 0}
						<Empty icon="template" title="No VM templates yet">
							Create one from the CLI, from scratch or from a VM you have set up:
							<CodeBlock
								code={'qarax vm-template create --name small --hypervisor cloud_hv --vcpus 1 --memory 536870912\nqarax vm-template create --name web --from-vm web-01'}
							/>
						</Empty>
					{:else}
						<table class="pf-v6-c-table pf-m-grid-md" aria-label="VM templates">
							<thead class="pf-v6-c-table__thead">
								<tr class="pf-v6-c-table__tr">
									<th class="pf-v6-c-table__th" scope="col">Name</th>
									<th class="pf-v6-c-table__th" scope="col">Boots</th>
									<th class="pf-v6-c-table__th" scope="col">Size</th>
									<th class="pf-v6-c-table__th" scope="col">Hypervisor</th>
									<th class="pf-v6-c-table__th" scope="col">Network</th>
									<th class="pf-v6-c-table__th" scope="col">Sandbox pool</th>
								</tr>
							</thead>
							<tbody class="pf-v6-c-table__tbody">
								{#each templates as template (template.id)}
									{@const size = templateSize(template)}
									<tr class="pf-v6-c-table__tr">
										<td class="pf-v6-c-table__td" data-label="Name">
											<a href={resolve('/templates/[id]', { id: template.id })}>{template.name}</a>
											{#if template.description}
												<div class="qx-subtle">{template.description}</div>
											{/if}
										</td>
										<td class="pf-v6-c-table__td boots" data-label="Boots"
											>{capitalize(templateBoots(template, names))}</td
										>
										<td class="pf-v6-c-table__td" data-label="Size">
											{#if size}{size}{:else}<span class="qx-subtle">Not set</span>{/if}
										</td>
										<td class="pf-v6-c-table__td" data-label="Hypervisor">
											{#if template.hypervisor}{HYPERVISOR[template.hypervisor]}{:else}<span
													class="qx-subtle">Not set</span
												>{/if}
										</td>
										<td class="pf-v6-c-table__td" data-label="Network">
											{#if template.network_id}
												<a href={resolve('/networks/[id]', { id: template.network_id })}
													>{networkNames.get(template.network_id) ?? template.network_id}</a
												>
											{:else}
												<span class="qx-subtle">None</span>
											{/if}
										</td>
										<td class="pf-v6-c-table__td" data-label="Sandbox pool">
											{#if pooled.has(template.id)}Yes{:else}<span class="qx-subtle">No</span>{/if}
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

<style>
	.boots {
		overflow-wrap: anywhere;
	}
</style>
