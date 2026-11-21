<script lang="ts">
	import { resolve } from '$app/paths';
	import { allHosts, allVms, type Vm } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { VM_STATUS } from '#lib/status.ts';
	import { HYPERVISOR, bytes } from '#lib/format.ts';
	import CodeBlock from '#lib/components/CodeBlock.svelte';
	import Empty from '#lib/components/Empty.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import SearchInput from '#lib/components/SearchInput.svelte';
	import Status from '#lib/components/Status.svelte';

	const list = new Remote(async () => {
		const [vms, hosts] = await Promise.all([allVms(), allHosts()]);
		return { vms, hostNames: new Map(hosts.map((host) => [host.id, host.name])) };
	});
	keepFresh(list.refresh);

	let filter = $state('');

	/** Every word of the filter is in the VM's name or one of its tags. */
	function matches(vm: Vm, query: string): boolean {
		const haystack = [vm.name, ...vm.tags].map((s) => s.toLowerCase());
		return query
			.toLowerCase()
			.split(/\s+/)
			.filter(Boolean)
			.every((word) => haystack.some((s) => s.includes(word)));
	}
</script>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Virtual machines</h1>
		<p class="pf-v6-c-content--p qx-subtle">Every VM qarax manages, on all hosts.</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={list} what="virtual machines">
			{#snippet children({ vms, hostNames })}
				{#if vms.length === 0}
					<div>
						<Empty icon="virtualMachine" title="No virtual machines yet">
							A VM boots from an OCI image, a kernel or a disk on one of your hosts. Create one
							here, or from the CLI:
							<CodeBlock
								code={'qarax vm create --name my-first-vm --vcpus 1 --memory 1GiB --image-ref public.ecr.aws/docker/library/alpine:latest\nqarax vm start my-first-vm'}
							/>
							{#snippet footer()}
								<div class="pf-v6-c-empty-state__actions">
									<a class="pf-v6-c-button pf-m-primary" href={resolve('/vms/new')}>
										<span class="pf-v6-c-button__text">Create VM</span>
									</a>
								</div>
							{/snippet}
						</Empty>
					</div>
				{:else}
					{@const shown = vms.filter((vm) => matches(vm, filter))}
					<div>
						<div class="pf-v6-c-toolbar">
							<div class="pf-v6-c-toolbar__content">
								<div class="pf-v6-c-toolbar__content-section">
									<div class="pf-v6-c-toolbar__item search">
										<SearchInput bind:value={filter} label="Filter by name or tag" />
									</div>
									<div class="pf-v6-c-toolbar__item">
										<a class="pf-v6-c-button pf-m-primary" href={resolve('/vms/new')}>
											<span class="pf-v6-c-button__text">Create VM</span>
										</a>
									</div>
									<div class="pf-v6-c-toolbar__item pf-m-align-end qx-subtle count">
										{shown.length === vms.length
											? `${vms.length} ${vms.length === 1 ? 'VM' : 'VMs'}`
											: `${shown.length} of ${vms.length} VMs`}
									</div>
								</div>
							</div>
						</div>
						{#if shown.length}
							<table class="pf-v6-c-table pf-m-grid-md" aria-label="Virtual machines">
								<thead class="pf-v6-c-table__thead">
									<tr class="pf-v6-c-table__tr">
										<th class="pf-v6-c-table__th" scope="col">Name</th>
										<th class="pf-v6-c-table__th" scope="col">Status</th>
										<th class="pf-v6-c-table__th" scope="col">Host</th>
										<th class="pf-v6-c-table__th" scope="col">vCPUs</th>
										<th class="pf-v6-c-table__th" scope="col">Memory</th>
										<th class="pf-v6-c-table__th" scope="col">Hypervisor</th>
										<th class="pf-v6-c-table__th" scope="col">Boots from</th>
									</tr>
								</thead>
								<tbody class="pf-v6-c-table__tbody">
									{#each shown as vm (vm.id)}
										<tr class="pf-v6-c-table__tr">
											<td class="pf-v6-c-table__td" data-label="Name">
												<a href={resolve('/vms/[id]', { id: vm.id })}>{vm.name}</a>
												{#if vm.tags.length}
													<div class="pf-v6-c-label-group tags">
														<div class="pf-v6-c-label-group__main">
															<ul class="pf-v6-c-label-group__list" role="list" aria-label="Tags">
																{#each vm.tags as tag (tag)}
																	<li class="pf-v6-c-label-group__list-item">
																		<span class="pf-v6-c-label pf-m-compact pf-m-outline">
																			<span class="pf-v6-c-label__content">
																				<span class="pf-v6-c-label__text">{tag}</span>
																			</span>
																		</span>
																	</li>
																{/each}
															</ul>
														</div>
													</div>
												{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="Status"
												><Status look={VM_STATUS[vm.status]} /></td
											>
											<td class="pf-v6-c-table__td" data-label="Host">
												{#if !vm.host_id}
													<span class="qx-subtle">Not placed yet</span>
												{:else if hostNames.has(vm.host_id)}
													{hostNames.get(vm.host_id)}
												{:else}
													<span class="qx-mono qx-subtle">{vm.host_id}</span>
												{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="vCPUs">{vm.boot_vcpus}</td>
											<td class="pf-v6-c-table__td" data-label="Memory">{bytes(vm.memory_size)}</td>
											<td class="pf-v6-c-table__td" data-label="Hypervisor"
												>{HYPERVISOR[vm.hypervisor]}</td
											>
											<td class="pf-v6-c-table__td" data-label="Boots from">
												{#if vm.image_ref}
													<span class="qx-mono">{vm.image_ref}</span>
												{:else}
													{vm.boot_mode === 'firmware' ? 'Firmware' : 'Kernel'}
												{/if}
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						{:else}
							<Empty icon="search" title="No matching VMs">
								No VM's name or tags match “{filter}”.
								{#snippet footer()}
									<div class="pf-v6-c-empty-state__actions">
										<button
											class="pf-v6-c-button pf-m-link"
											type="button"
											onclick={() => (filter = '')}
										>
											<span class="pf-v6-c-button__text">Clear the filter</span>
										</button>
									</div>
								{/snippet}
							</Empty>
						{/if}
					</div>
				{/if}
			{/snippet}
		</Loaded>
	</div>
</section>

<style>
	.count {
		align-self: center;
	}

	.search {
		min-width: min(22rem, 100%);
	}

	.tags {
		margin-top: var(--pf-t--global--spacer--xs);
	}

	.qx-mono {
		overflow-wrap: anywhere;
	}
</style>
