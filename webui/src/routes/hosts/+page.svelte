<script lang="ts">
	import { allVms, hostsWithCapacity, type Host } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { HOST_STATUS } from '#lib/status.ts';
	import { ago, bytes } from '#lib/format.ts';
	import CapacityBar from '#lib/components/CapacityBar.svelte';
	import CodeBlock from '#lib/components/CodeBlock.svelte';
	import Empty from '#lib/components/Empty.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Status from '#lib/components/Status.svelte';

	const list = new Remote(async () => {
		const [units, vms] = await Promise.all([hostsWithCapacity(), allVms()]);
		return units.map((unit) => ({
			...unit,
			vmCount: vms.filter((vm) => vm.host_id === unit.host.id).length
		}));
	});
	keepFresh(list.refresh);

	/** Why a host isn't simply up, in a sentence; null when it is. */
	function why(host: Host): string | null {
		switch (host.status) {
			case 'down':
				return `Not answering at ${host.address}:${host.port}.`;
			case 'installation_failed':
				return host.installation_error ?? 'Its last deploy or upgrade failed.';
			case 'maintenance':
				return 'Takes no new VMs until maintenance ends.';
			case 'installing':
				return 'A deploy or upgrade is running.';
			case 'unknown':
			case 'initializing':
				return "qarax hasn't heard from it yet.";
			case 'up':
				return null;
		}
	}
</script>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Hosts</h1>
		<p class="pf-v6-c-content--p qx-subtle">The machines your VMs run on, and how full they are.</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={list} what="hosts">
			{#snippet children(units)}
				<div>
					{#if units.length === 0}
						<Empty icon="server" title="No hosts yet">
							A host is a machine with KVM that runs your VMs through the qarax-node service.
							Register one and deploy the node to it from the CLI:
							<CodeBlock
								code={'qarax host add --name node-01 --address 10.0.0.42 --user root\nqarax host deploy node-01 --image ghcr.io/qarax/qarax-vmm-host:vX.Y.Z --ssh-key ~/.ssh/id_ed25519'}
							/>
							docs/INSTALL.md walks through the rest, its certificates included.
						</Empty>
					{:else}
						<table class="pf-v6-c-table pf-m-compact pf-m-grid-lg" aria-label="Hosts">
							<thead class="pf-v6-c-table__thead">
								<tr class="pf-v6-c-table__tr">
									<th class="pf-v6-c-table__th" scope="col">Name</th>
									<th class="pf-v6-c-table__th" scope="col">Status</th>
									<th class="pf-v6-c-table__th" scope="col">VMs</th>
									<th class="pf-v6-c-table__th" scope="col">vCPUs</th>
									<th class="pf-v6-c-table__th" scope="col">Memory</th>
									<th class="pf-v6-c-table__th" scope="col">Disk used</th>
									<th class="pf-v6-c-table__th" scope="col">Load</th>
								</tr>
							</thead>
							<tbody class="pf-v6-c-table__tbody">
								{#each units as { host, capacity, vmCount } (host.id)}
									{@const reason = why(host)}
									<tr class="pf-v6-c-table__tr">
										<td class="pf-v6-c-table__td" data-label="Name">
											<strong>{host.name}</strong>
											<div class="qx-mono qx-subtle">{host.address}:{host.port}</div>
											<div class="qx-subtle">
												{[host.architecture, host.node_version && `qarax-node ${host.node_version}`]
													.filter(Boolean)
													.join(', ')}
											</div>
											{#if host.update_available}
												<span class="pf-v6-c-label pf-m-compact pf-m-blue">
													<span class="pf-v6-c-label__content">
														<span class="pf-v6-c-label__text">Update available</span>
													</span>
												</span>
											{/if}
										</td>
										<td class="pf-v6-c-table__td" data-label="Status">
											<Status look={HOST_STATUS[host.status]} />
											{#if reason}
												<div class="reason" class:fault={HOST_STATUS[host.status].tone === 'fault'}>
													{reason}
												</div>
											{/if}
											<div class="reported qx-subtle">
												{host.resources_updated_at
													? `Reported ${ago(host.resources_updated_at)}`
													: 'Never reported'}
											</div>
										</td>
										<td class="pf-v6-c-table__td" data-label="VMs">{vmCount}</td>
										<td class="pf-v6-c-table__td bar" data-label="vCPUs">
											<CapacityBar
												label="vCPUs allocated"
												used={capacity.allocated_vcpus}
												total={capacity.total_cpus}
											/>
										</td>
										<td class="pf-v6-c-table__td bar" data-label="Memory">
											<CapacityBar
												label="Memory allocated"
												used={capacity.allocated_memory_bytes}
												total={capacity.total_memory_bytes}
												format={bytes}
											/>
										</td>
										<td class="pf-v6-c-table__td bar" data-label="Disk used">
											<CapacityBar
												label="Disk used"
												used={capacity.disk_total_bytes != null &&
												capacity.disk_available_bytes != null
													? capacity.disk_total_bytes - capacity.disk_available_bytes
													: 0}
												total={capacity.disk_total_bytes}
												format={bytes}
											/>
										</td>
										<td class="pf-v6-c-table__td" data-label="Load">
											{#if host.load_average != null}{host.load_average.toFixed(2)}{:else}<span
													class="qx-subtle">Not reported</span
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

<style>
	.reason {
		margin-top: var(--pf-t--global--spacer--xs);
		max-width: 20rem;
		font-size: var(--pf-t--global--font--size--body--sm);
		color: var(--pf-t--global--text--color--subtle);
	}

	.reason.fault {
		color: var(--pf-t--global--text--color--status--danger--default);
	}

	.bar {
		min-width: 9rem;
	}

	.reported {
		margin-top: var(--pf-t--global--spacer--xs);
		font-size: var(--pf-t--global--font--size--body--sm);
		white-space: nowrap;
	}

	td :global(.qx-subtle) {
		white-space: nowrap;
	}
</style>
