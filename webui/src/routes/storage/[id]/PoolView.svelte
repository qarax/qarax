<script lang="ts">
	import { untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import {
		allStorageObjects,
		allTransfers,
		allVms,
		api,
		call,
		type StoragePoolType
	} from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { POOL_STATUS, TRANSFER_STATUS } from '#lib/status.ts';
	import { OBJECT_TYPE, POOL_TYPE, ago, bytes, poolLocation } from '#lib/format.ts';
	import CapacityBar from '#lib/components/CapacityBar.svelte';
	import Fact from '#lib/components/Fact.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Status from '#lib/components/Status.svelte';
	import StatusLabel from '#lib/components/StatusLabel.svelte';

	let { id }: { id: string } = $props();

	// The page re-creates this view for another pool ({#key}), so the id is fixed.
	const poolId = untrack(() => id);
	const path = { params: { path: { pool_id: poolId } } };

	const detail = new Remote(async () => {
		const [pool, hosts, objects, transfers, vms] = await Promise.all([
			call(api.GET('/v1/storage-pools/{pool_id}', path)),
			call(api.GET('/v1/storage-pools/{pool_id}/hosts', path)),
			allStorageObjects({ poolId }),
			allTransfers(poolId),
			allVms()
		]);
		return { pool, hosts, objects, transfers, vmNames: new Map(vms.map((vm) => [vm.id, vm.name])) };
	});
	// Pools don't change with VMs' statuses, but their objects do as VMs come and go.
	keepFresh(detail.refresh);

	/** What a pool of each type is, for someone who hasn't met one. */
	const ABOUT: Record<StoragePoolType, string> = {
		local: 'A directory on each host it is attached to. What it holds stays on that host.',
		nfs: 'An NFS export each host it is attached to mounts. Its disks can follow a VM to another host.',
		overlay_bd:
			'A registry VMs boot OCI images from, reading blocks as the guest needs them instead of downloading the image first.',
		block: 'LUNs on an iSCSI target, which each host it is attached to logs in to.'
	};
</script>

<section class="pf-v6-c-page__main-breadcrumb">
	<div class="pf-v6-c-page__main-body">
		<nav class="pf-v6-c-breadcrumb" aria-label="Breadcrumb">
			<ol class="pf-v6-c-breadcrumb__list" role="list">
				<li class="pf-v6-c-breadcrumb__item">
					<a href={resolve('/storage')} class="pf-v6-c-breadcrumb__link">Storage pools</a>
				</li>
				<li class="pf-v6-c-breadcrumb__item">
					<span class="pf-v6-c-breadcrumb__item-divider"><Icon name="chevronRight" /></span>
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page">
						{detail.state.kind === 'ready' || detail.state.kind === 'stale'
							? detail.state.data.pool.name
							: 'Storage pool'}
					</span>
				</li>
			</ol>
		</nav>
	</div>
</section>

<Loaded remote={detail} what="this storage pool">
	{#snippet children({ pool, hosts, objects, transfers, vmNames })}
		{@const location = poolLocation(pool)}
		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="title">
					<h1 class="pf-v6-c-title pf-m-2xl">{pool.name}</h1>
					<StatusLabel look={POOL_STATUS[pool.status]} />
				</div>
				<p class="pf-v6-c-content--p qx-subtle">{ABOUT[pool.pool_type]}</p>
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
									<Fact term="Type">{POOL_TYPE[pool.pool_type]}</Fact>
									<Fact term="Location">
										{#if location}<span class="qx-mono wrap">{location}</span>{:else}<span
												class="qx-subtle">Not set</span
											>{/if}
									</Fact>
									<Fact term="Allocated">
										{#if pool.capacity_bytes}
											<CapacityBar
												label="Allocated"
												used={pool.allocated_bytes ?? 0}
												total={pool.capacity_bytes}
												format={bytes}
											/>
										{:else}
											{bytes(pool.allocated_bytes ?? 0)}
											<span class="qx-subtle">(no capacity set)</span>
										{/if}
									</Fact>
									<Fact term="ID"><span class="qx-mono">{pool.id}</span></Fact>
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
									<p class="qx-subtle intro">VMs on these hosts can use what this pool holds.</p>
									<ul class="pf-v6-c-list pf-m-plain" role="list">
										{#each hosts as host (host.host_id)}
											<li>{host.host_name}</li>
										{/each}
									</ul>
								{:else}
									<p class="qx-subtle">
										No host can use it yet. Attach one with <code
											>qarax storage-pool attach-host {pool.name} &lt;host&gt;</code
										>, or every host that is up with <code>--all</code>.
									</p>
								{/if}
							</div>
						</div>
					</div>

					<div class="pf-v6-l-grid__item pf-m-12-col">
						<div class="pf-v6-c-card">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Storage objects</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if objects.length === 0}
									<p class="qx-subtle">
										It holds nothing yet. Disks, kernels and images show up here as they are created
										in it.
									</p>
								{:else}
									<table
										class="pf-v6-c-table pf-m-compact pf-m-grid-md"
										aria-label="Storage objects"
									>
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Name</th>
												<th class="pf-v6-c-table__th" scope="col">Type</th>
												<th class="pf-v6-c-table__th" scope="col">Size</th>
												<th class="pf-v6-c-table__th" scope="col">Deleted with</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each objects as object (object.id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td" data-label="Name">
														<span class="wrap">{object.name}</span>
													</td>
													<td class="pf-v6-c-table__td" data-label="Type"
														>{OBJECT_TYPE[object.object_type]}</td
													>
													<td class="pf-v6-c-table__td" data-label="Size"
														>{bytes(object.size_bytes)}</td
													>
													<td class="pf-v6-c-table__td" data-label="Deleted with">
														{#if object.owner_vm_id}
															<a href={resolve('/vms/[id]', { id: object.owner_vm_id })}
																>{vmNames.get(object.owner_vm_id) ?? object.owner_vm_id}</a
															>
														{:else}
															<span class="qx-subtle">Kept until you delete it</span>
														{/if}
													</td>
												</tr>
											{/each}
										</tbody>
									</table>
								{/if}
							</div>
						</div>
					</div>

					{#if transfers.length}
						<div class="pf-v6-l-grid__item pf-m-12-col">
							<div class="pf-v6-c-card">
								<div class="pf-v6-c-card__title">
									<h2 class="pf-v6-c-card__title-text">Transfers</h2>
								</div>
								<div class="pf-v6-c-card__body">
									<p class="qx-subtle intro">Files copied into this pool from a URL or a path.</p>
									<table class="pf-v6-c-table pf-m-compact pf-m-grid-md" aria-label="Transfers">
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Name</th>
												<th class="pf-v6-c-table__th" scope="col">Status</th>
												<th class="pf-v6-c-table__th" scope="col">Copied</th>
												<th class="pf-v6-c-table__th" scope="col">From</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each transfers as transfer (transfer.id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td" data-label="Name">
														{transfer.name}
														<div class="qx-subtle">
															{OBJECT_TYPE[transfer.object_type]}{#if transfer.created_at}, started
																{ago(transfer.created_at)}{/if}
														</div>
													</td>
													<td class="pf-v6-c-table__td" data-label="Status">
														<Status look={TRANSFER_STATUS[transfer.status]} />
														{#if transfer.status === 'failed' && transfer.error_message}
															<div class="error">{transfer.error_message}</div>
														{/if}
													</td>
													<td class="pf-v6-c-table__td bar" data-label="Copied">
														{#if transfer.total_bytes}
															<CapacityBar
																label="Copied"
																used={transfer.transferred_bytes}
																total={transfer.total_bytes}
																format={bytes}
															/>
														{:else}
															{bytes(transfer.transferred_bytes)}
														{/if}
													</td>
													<td class="pf-v6-c-table__td" data-label="From">
														<span class="qx-mono wrap">{transfer.source}</span>
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

	.wrap {
		overflow-wrap: anywhere;
	}

	.bar {
		min-width: 10rem;
	}

	.error {
		margin-top: var(--pf-t--global--spacer--xs);
		font-size: var(--pf-t--global--font--size--body--sm);
		color: var(--pf-t--global--text--color--status--danger--default);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
</style>
