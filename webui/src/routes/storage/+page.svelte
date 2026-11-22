<script lang="ts">
	import { resolve } from '$app/paths';
	import { allStorageObjects, allStoragePools } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { POOL_STATUS } from '#lib/status.ts';
	import { POOL_TYPE, bytes, poolLocation } from '#lib/format.ts';
	import CapacityBar from '#lib/components/CapacityBar.svelte';
	import CodeBlock from '#lib/components/CodeBlock.svelte';
	import Empty from '#lib/components/Empty.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Status from '#lib/components/Status.svelte';

	const list = new Remote(async () => {
		const [pools, objects] = await Promise.all([allStoragePools(), allStorageObjects({})]);
		return pools.map((pool) => ({
			pool,
			objectCount: objects.filter((object) => object.storage_pool_id === pool.id).length
		}));
	});
	keepFresh(list.refresh);
</script>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Storage pools</h1>
		<p class="pf-v6-c-content--p qx-subtle">
			Where disks, kernels, images and backups live, and how full each place is.
		</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={list} what="storage pools">
			{#snippet children(units)}
				<div>
					{#if units.length === 0}
						<Empty icon="storage" title="No storage pools yet">
							A storage pool is a place for VM disks and images: a directory on a host, an NFS
							export, an iSCSI target, or a registry to boot OCI images from. Create one and attach
							it to hosts here, or from the CLI:
							<CodeBlock
								code={'qarax storage-pool create --name images --pool-type local --path /var/lib/qarax/images --host node-01'}
							/>
							{#snippet footer()}
								<div class="pf-v6-c-empty-state__actions">
									<a class="pf-v6-c-button pf-m-primary" href={resolve('/storage/new')}>
										<span class="pf-v6-c-button__text">Create storage pool</span>
									</a>
								</div>
							{/snippet}
						</Empty>
					{:else}
						<div class="pf-v6-c-toolbar">
							<div class="pf-v6-c-toolbar__content">
								<div class="pf-v6-c-toolbar__content-section">
									<div class="pf-v6-c-toolbar__item">
										<a class="pf-v6-c-button pf-m-primary" href={resolve('/storage/new')}>
											<span class="pf-v6-c-button__text">Create storage pool</span>
										</a>
									</div>
								</div>
							</div>
						</div>
						<table class="pf-v6-c-table pf-m-grid-md" aria-label="Storage pools">
							<thead class="pf-v6-c-table__thead">
								<tr class="pf-v6-c-table__tr">
									<th class="pf-v6-c-table__th" scope="col">Name</th>
									<th class="pf-v6-c-table__th" scope="col">Status</th>
									<th class="pf-v6-c-table__th" scope="col">Type</th>
									<th class="pf-v6-c-table__th" scope="col">Location</th>
									<th class="pf-v6-c-table__th" scope="col">Allocated</th>
									<th class="pf-v6-c-table__th" scope="col">Objects</th>
								</tr>
							</thead>
							<tbody class="pf-v6-c-table__tbody">
								{#each units as { pool, objectCount } (pool.id)}
									{@const location = poolLocation(pool)}
									<tr class="pf-v6-c-table__tr">
										<td class="pf-v6-c-table__td" data-label="Name">
											<a href={resolve('/storage/[id]', { id: pool.id })}>{pool.name}</a>
										</td>
										<td class="pf-v6-c-table__td" data-label="Status"
											><Status look={POOL_STATUS[pool.status]} /></td
										>
										<td class="pf-v6-c-table__td" data-label="Type">{POOL_TYPE[pool.pool_type]}</td>
										<td class="pf-v6-c-table__td" data-label="Location">
											{#if location}<span class="qx-mono location">{location}</span>{:else}<span
													class="qx-subtle">Not set</span
												>{/if}
										</td>
										<td class="pf-v6-c-table__td bar" data-label="Allocated">
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
										</td>
										<td class="pf-v6-c-table__td" data-label="Objects">{objectCount}</td>
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
	.location {
		overflow-wrap: anywhere;
	}

	.bar {
		min-width: 10rem;
	}
</style>
