<script lang="ts">
	import { untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import { allHosts, allVms, api, call, hostCapacity } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { HOST_STATUS, VM_STATUS } from '#lib/status.ts';
	import { HYPERVISOR, ago, bytes } from '#lib/format.ts';
	import { hostSituation } from '#lib/hosts.ts';
	import { jobFinished } from '#lib/vm-actions.ts';
	import CapacityBar from '#lib/components/CapacityBar.svelte';
	import ConfirmModal from '#lib/components/ConfirmModal.svelte';
	import Fact from '#lib/components/Fact.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import Status from '#lib/components/Status.svelte';
	import StatusLabel from '#lib/components/StatusLabel.svelte';

	let { id }: { id: string } = $props();

	// The page re-creates this view for another host ({#key}), so the id is fixed.
	const hostId = untrack(() => id);
	const path = { params: { path: { host_id: hostId } } };

	const detail = new Remote(async () => {
		// The API has no single-host read; the list carries every field.
		const [hosts, capacity, vms, gpus, numa] = await Promise.all([
			allHosts(),
			hostCapacity(hostId),
			allVms(),
			call(api.GET('/v1/hosts/{host_id}/gpus', path)),
			call(api.GET('/v1/hosts/{host_id}/numa', path))
		]);
		const host = hosts.find((host) => host.id === hostId);
		if (!host) throw new Error('qarax has no host with this ID. It may have been removed.');
		return {
			host,
			capacity,
			vms: vms.filter((vm) => vm.host_id === hostId),
			vmNames: new Map(vms.map((vm) => [vm.id, vm.name])),
			gpus,
			numa
		};
	});
	// VMs arrive, leave and change status on it; its metrics come every few seconds.
	keepFresh(detail.refresh);

	type ActionState =
		| { kind: 'idle' }
		| { kind: 'confirming-evacuation' }
		| { kind: 'running'; action: 'maintenance' | 'end-maintenance' | 'evacuate' }
		| { kind: 'failed'; message: string };
	let action: ActionState = $state({ kind: 'idle' });

	const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

	async function setStatus(status: 'maintenance' | 'up') {
		action = { kind: 'running', action: status === 'up' ? 'end-maintenance' : 'maintenance' };
		try {
			await call(api.PATCH('/v1/hosts/{host_id}', { ...path, body: { status } }));
			action = { kind: 'idle' };
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
		}
		void detail.refresh();
	}

	/** Moves its VMs away one at a time; the buttons wait for the whole job. */
	async function evacuate() {
		action = { kind: 'running', action: 'evacuate' };
		try {
			const { job_id } = await call(api.POST('/v1/hosts/{host_id}/evacuate', path));
			void detail.refresh();
			await jobFinished(job_id);
			action = { kind: 'idle' };
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
		}
		void detail.refresh();
	}

	const running = (name: 'maintenance' | 'end-maintenance' | 'evacuate') =>
		action.kind === 'running' && action.action === name;
	const notReported = 'Not reported';
</script>

<section class="pf-v6-c-page__main-breadcrumb">
	<div class="pf-v6-c-page__main-body">
		<nav class="pf-v6-c-breadcrumb" aria-label="Breadcrumb">
			<ol class="pf-v6-c-breadcrumb__list" role="list">
				<li class="pf-v6-c-breadcrumb__item">
					<a href={resolve('/hosts')} class="pf-v6-c-breadcrumb__link">Hosts</a>
				</li>
				<li class="pf-v6-c-breadcrumb__item">
					<span class="pf-v6-c-breadcrumb__item-divider"><Icon name="chevronRight" /></span>
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page">
						{detail.state.kind === 'ready' || detail.state.kind === 'stale'
							? detail.state.data.host.name
							: 'Host'}
					</span>
				</li>
			</ol>
		</nav>
	</div>
</section>

<Loaded remote={detail} what="this host">
	{#snippet children({ host, capacity, vms, vmNames, gpus, numa })}
		{@const look = HOST_STATUS[host.status]}
		{@const busy = action.kind === 'running'}
		{@const labels = Object.entries(host.placement_labels ?? {})}
		{@const canEvacuate = (host.status === 'up' || host.status === 'maintenance') && vms.length > 0}
		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="pf-v6-l-split pf-m-gutter pf-m-wrap">
					<div class="pf-v6-l-split__item pf-m-fill">
						<div class="title">
							<h1 class="pf-v6-c-title pf-m-2xl">{host.name}</h1>
							<StatusLabel {look} />
						</div>
						<p
							class="pf-v6-c-content--p"
							class:qx-subtle={look.tone !== 'fault'}
							class:fault={look.tone === 'fault'}
						>
							{hostSituation(host)}
						</p>
					</div>
					<div class="pf-v6-l-split__item actions">
						{#if host.status === 'maintenance'}
							<button
								class="pf-v6-c-button pf-m-primary"
								type="button"
								disabled={busy}
								onclick={() => setStatus('up')}
							>
								<span class="pf-v6-c-button__text"
									>{running('end-maintenance') ? 'Ending maintenance…' : 'End maintenance'}</span
								>
							</button>
						{/if}
						{#if canEvacuate}
							<button
								class="pf-v6-c-button pf-m-secondary"
								type="button"
								disabled={busy}
								onclick={() => (action = { kind: 'confirming-evacuation' })}
							>
								<span class="pf-v6-c-button__text"
									>{running('evacuate') ? 'Evacuating…' : 'Evacuate'}</span
								>
							</button>
						{/if}
						{#if host.status === 'up'}
							<button
								class="pf-v6-c-button pf-m-secondary"
								type="button"
								disabled={busy}
								onclick={() => setStatus('maintenance')}
							>
								<span class="pf-v6-c-button__text"
									>{running('maintenance') ? 'Starting maintenance…' : 'Start maintenance'}</span
								>
							</button>
						{/if}
					</div>
				</div>
				{#if action.kind === 'failed'}
					<div class="failed">
						<Notice tone="danger" title="That didn't work"
							><span class="message">{action.message}</span></Notice
						>
					</div>
				{/if}
			</div>
		</section>

		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="pf-v6-l-grid pf-m-gutter">
					<div class="pf-v6-l-grid__item pf-m-12-col pf-m-6-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Machine</h2>
							</div>
							<div class="pf-v6-c-card__body">
								<dl class="pf-v6-c-description-list pf-m-horizontal">
									<Fact term="Address"><span class="qx-mono">{host.address}:{host.port}</span></Fact
									>
									<Fact term="Architecture">{host.architecture ?? notReported}</Fact>
									<Fact term="CPUs">{host.total_cpus ?? notReported}</Fact>
									<Fact term="Memory"
										>{host.total_memory_bytes != null
											? bytes(host.total_memory_bytes)
											: notReported}</Fact
									>
									<Fact term="Kernel">{host.kernel_version ?? notReported}</Fact>
									<Fact term="qarax-node">
										{host.node_version ?? notReported}
										{#if host.update_available}
											<span class="pf-v6-c-label pf-m-compact pf-m-blue">
												<span class="pf-v6-c-label__content">
													<span class="pf-v6-c-label__text">Update available</span>
												</span>
											</span>
										{/if}
									</Fact>
									<Fact term="Cloud Hypervisor">{host.cloud_hypervisor_version ?? notReported}</Fact
									>
									<Fact term="Firecracker">{host.firecracker_version ?? notReported}</Fact>
									{#if host.last_deployed_image}
										<Fact term="Image"
											><span class="qx-mono wrap">{host.last_deployed_image}</span></Fact
										>
									{/if}
									<Fact term="ID"><span class="qx-mono">{host.id}</span></Fact>
								</dl>
							</div>
						</div>
					</div>
					<div class="pf-v6-l-grid__item pf-m-12-col pf-m-6-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Capacity</h2>
							</div>
							<div class="pf-v6-c-card__body">
								<dl class="pf-v6-c-description-list pf-m-horizontal">
									<Fact term="vCPUs allocated">
										<CapacityBar
											label="vCPUs allocated"
											used={capacity.allocated_vcpus}
											total={capacity.total_cpus}
										/>
									</Fact>
									<Fact term="Memory allocated">
										<CapacityBar
											label="Memory allocated"
											used={capacity.allocated_memory_bytes}
											total={capacity.total_memory_bytes}
											format={bytes}
										/>
									</Fact>
									<Fact term="Memory in use">
										<CapacityBar
											label="Memory in use"
											used={host.total_memory_bytes != null && host.available_memory_bytes != null
												? host.total_memory_bytes - host.available_memory_bytes
												: 0}
											total={host.available_memory_bytes != null ? host.total_memory_bytes : null}
											format={bytes}
										/>
									</Fact>
									<Fact term="Disk used">
										<CapacityBar
											label="Disk used"
											used={capacity.disk_total_bytes != null &&
											capacity.disk_available_bytes != null
												? capacity.disk_total_bytes - capacity.disk_available_bytes
												: 0}
											total={capacity.disk_total_bytes}
											format={bytes}
										/>
									</Fact>
									<Fact term="Load average"
										>{host.load_average != null ? host.load_average.toFixed(2) : notReported}</Fact
									>
								</dl>
								<p class="qx-subtle reported">
									Allocated is what its VMs were given; in use is what the host's memory holds.
									{host.resources_updated_at
										? `Reported ${ago(host.resources_updated_at)}.`
										: 'It has never reported.'}
								</p>
							</div>
						</div>
					</div>

					<div class="pf-v6-l-grid__item pf-m-12-col">
						<div class="pf-v6-c-card">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Virtual machines</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if vms.length === 0}
									<p class="qx-subtle">None are placed on it.</p>
								{:else}
									<table
										class="pf-v6-c-table pf-m-compact pf-m-grid-md"
										aria-label="Virtual machines"
									>
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Name</th>
												<th class="pf-v6-c-table__th" scope="col">Status</th>
												<th class="pf-v6-c-table__th" scope="col">vCPUs</th>
												<th class="pf-v6-c-table__th" scope="col">Memory</th>
												<th class="pf-v6-c-table__th" scope="col">Hypervisor</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each vms as vm (vm.id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td" data-label="Name">
														<a href={resolve('/vms/[id]', { id: vm.id })}>{vm.name}</a>
													</td>
													<td class="pf-v6-c-table__td" data-label="Status"
														><Status look={VM_STATUS[vm.status]} /></td
													>
													<td class="pf-v6-c-table__td" data-label="vCPUs">{vm.boot_vcpus}</td>
													<td class="pf-v6-c-table__td" data-label="Memory"
														>{bytes(vm.memory_size)}</td
													>
													<td class="pf-v6-c-table__td" data-label="Hypervisor"
														>{HYPERVISOR[vm.hypervisor]}</td
													>
												</tr>
											{/each}
										</tbody>
									</table>
								{/if}
							</div>
						</div>
					</div>

					<div class="pf-v6-l-grid__item pf-m-12-col pf-m-6-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Placement</h2>
							</div>
							<div class="pf-v6-c-card__body">
								<dl class="pf-v6-c-description-list pf-m-horizontal">
									<Fact term="Reservation class"
										>{#if host.reservation_class}{host.reservation_class}{:else}<span
												class="qx-subtle">None</span
											>{/if}</Fact
									>
									<Fact term="Labels">
										{#if labels.length}
											<div class="pf-v6-c-label-group">
												<div class="pf-v6-c-label-group__main">
													<ul class="pf-v6-c-label-group__list" role="list" aria-label="Labels">
														{#each labels as [key, value] (key)}
															<li class="pf-v6-c-label-group__list-item">
																<span class="pf-v6-c-label pf-m-compact pf-m-outline">
																	<span class="pf-v6-c-label__content">
																		<span class="pf-v6-c-label__text qx-mono">{key}={value}</span>
																	</span>
																</span>
															</li>
														{/each}
													</ul>
												</div>
											</div>
										{:else}
											<span class="qx-subtle">None</span>
										{/if}
									</Fact>
								</dl>
								<p class="qx-subtle reported">
									The scheduler matches these against VMs' placement policies. Change them with
									<code>qarax host placement set {host.name}</code>.
								</p>
							</div>
						</div>
					</div>

					{#if numa.length}
						<div class="pf-v6-l-grid__item pf-m-12-col pf-m-6-col-on-xl">
							<div class="pf-v6-c-card pf-m-full-height">
								<div class="pf-v6-c-card__title">
									<h2 class="pf-v6-c-card__title-text">NUMA nodes</h2>
								</div>
								<div class="pf-v6-c-card__body">
									<table class="pf-v6-c-table pf-m-compact" aria-label="NUMA nodes">
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Node</th>
												<th class="pf-v6-c-table__th" scope="col">CPUs</th>
												<th class="pf-v6-c-table__th" scope="col">Memory</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each numa as node (node.id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td" data-label="Node">{node.node_id}</td>
													<td class="pf-v6-c-table__td qx-mono" data-label="CPUs"
														>{node.cpu_list}</td
													>
													<td class="pf-v6-c-table__td" data-label="Memory"
														>{node.memory_bytes != null
															? bytes(node.memory_bytes)
															: notReported}</td
													>
												</tr>
											{/each}
										</tbody>
									</table>
								</div>
							</div>
						</div>
					{/if}

					{#if gpus.length}
						<div class="pf-v6-l-grid__item pf-m-12-col">
							<div class="pf-v6-c-card">
								<div class="pf-v6-c-card__title">
									<h2 class="pf-v6-c-card__title-text">GPUs</h2>
								</div>
								<div class="pf-v6-c-card__body">
									<table class="pf-v6-c-table pf-m-compact pf-m-grid-md" aria-label="GPUs">
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">PCI address</th>
												<th class="pf-v6-c-table__th" scope="col">Model</th>
												<th class="pf-v6-c-table__th" scope="col">Memory</th>
												<th class="pf-v6-c-table__th" scope="col">NUMA node</th>
												<th class="pf-v6-c-table__th" scope="col">Used by</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each gpus as gpu (gpu.id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td qx-mono" data-label="PCI address"
														>{gpu.pci_address}</td
													>
													<td class="pf-v6-c-table__td" data-label="Model"
														>{[gpu.vendor, gpu.model].filter(Boolean).join(' ') || notReported}</td
													>
													<td class="pf-v6-c-table__td" data-label="Memory"
														>{gpu.vram_bytes != null ? bytes(gpu.vram_bytes) : notReported}</td
													>
													<td class="pf-v6-c-table__td" data-label="NUMA node">{gpu.numa_node}</td>
													<td class="pf-v6-c-table__td" data-label="Used by">
														{#if gpu.vm_id}
															<a href={resolve('/vms/[id]', { id: gpu.vm_id })}
																>{vmNames.get(gpu.vm_id) ?? gpu.vm_id}</a
															>
														{:else}
															<span class="qx-subtle">Free</span>
														{/if}
													</td>
												</tr>
											{/each}
										</tbody>
									</table>
								</div>
							</div>
						</div>
					{/if}
				</div>
			</div>
		</section>

		{#if action.kind === 'confirming-evacuation'}
			<ConfirmModal
				title="Evacuate {host.name}?"
				confirm="Evacuate"
				tone="warning"
				onconfirm={evacuate}
				oncancel={() => (action = { kind: 'idle' })}
			>
				qarax moves {vms.length === 1 ? 'its VM' : `its ${vms.length} VMs`} to other hosts one at a time
				while they keep running, and puts {host.name} in maintenance so it takes no new ones. Only running
				and paused VMs can move.
			</ConfirmModal>
		{/if}
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

	.actions {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-start;
		gap: var(--pf-t--global--spacer--sm);
	}

	.fault {
		color: var(--pf-t--global--text--color--status--danger--default);
	}

	.failed {
		margin-top: var(--pf-t--global--spacer--md);
	}

	/* Errors from a node can span lines (a command's output, say). */
	.message,
	.fault {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.reported {
		margin-top: var(--pf-t--global--spacer--md);
		font-size: var(--pf-t--global--font--size--body--sm);
	}

	.wrap {
		overflow-wrap: anywhere;
	}
</style>
