<script lang="ts">
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { allHosts, allStorageObjects, allTransfers, allVms, api, call } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { POOL_STATUS, TRANSFER_STATUS } from '#lib/status.ts';
	import { OBJECT_TYPE, POOL_ABOUT, POOL_TYPE, ago, bytes, poolLocation } from '#lib/format.ts';
	import CapacityBar from '#lib/components/CapacityBar.svelte';
	import ConfirmModal from '#lib/components/ConfirmModal.svelte';
	import Fact from '#lib/components/Fact.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import FormControl from '#lib/components/FormControl.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import Status from '#lib/components/Status.svelte';
	import StatusLabel from '#lib/components/StatusLabel.svelte';

	let { id }: { id: string } = $props();

	// The page re-creates this view for another pool ({#key}), so the id is fixed.
	const poolId = untrack(() => id);
	const path = { params: { path: { pool_id: poolId } } };

	const detail = new Remote(async () => {
		const [pool, hosts, objects, transfers, vms, allHostList] = await Promise.all([
			call(api.GET('/v1/storage-pools/{pool_id}', path)),
			call(api.GET('/v1/storage-pools/{pool_id}/hosts', path)),
			allStorageObjects({ poolId }),
			allTransfers(poolId),
			allVms(),
			allHosts()
		]);
		const attached = new Set(hosts.map((host) => host.host_id));
		return {
			pool,
			hosts,
			objects,
			transfers,
			vmNames: new Map(vms.map((vm) => [vm.id, vm.name])),
			// The node mounts or logs in to the pool as it is attached, so it has to be up.
			attachable: allHostList.filter((host) => host.status === 'up' && !attached.has(host.id))
		};
	});
	// Pools don't change with VMs' statuses, but their objects do as VMs come and go.
	keepFresh(detail.refresh);

	type Attached = { host_id: string; host_name: string };
	type ActionState =
		| { kind: 'idle' }
		| { kind: 'confirming-delete' }
		| { kind: 'confirming-detach'; host: Attached }
		| { kind: 'running'; action: 'attach' | 'detach' | 'delete' }
		| { kind: 'failed'; message: string };
	let action: ActionState = $state({ kind: 'idle' });
	let attachHostId = $state('');

	const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

	async function run(name: 'attach' | 'detach' | 'delete', request: () => Promise<unknown>) {
		action = { kind: 'running', action: name };
		try {
			await request();
			action = { kind: 'idle' };
			return true;
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
			return false;
		}
	}

	async function attachHost(event: SubmitEvent) {
		event.preventDefault();
		const host_id = attachHostId;
		if (!host_id) return;
		const done = await run('attach', () =>
			call(api.POST('/v1/storage-pools/{pool_id}/hosts', { ...path, body: { host_id } }))
		);
		void detail.refresh();
		if (done) attachHostId = '';
	}

	async function detachHost(host: Attached) {
		await run('detach', () =>
			call(
				api.DELETE('/v1/storage-pools/{pool_id}/hosts/{host_id}', {
					params: { path: { pool_id: poolId, host_id: host.host_id } }
				})
			)
		);
		void detail.refresh();
	}

	async function remove() {
		if (await run('delete', () => call(api.DELETE('/v1/storage-pools/{pool_id}', path))))
			await goto(resolve('/storage'));
		else void detail.refresh();
	}
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
	{#snippet children({ pool, hosts, objects, transfers, vmNames, attachable })}
		{@const location = poolLocation(pool)}
		{@const busy = action.kind === 'running'}
		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="pf-v6-l-split pf-m-gutter pf-m-wrap">
					<div class="pf-v6-l-split__item pf-m-fill">
						<div class="title">
							<h1 class="pf-v6-c-title pf-m-2xl">{pool.name}</h1>
							<StatusLabel look={POOL_STATUS[pool.status]} />
						</div>
						<p class="pf-v6-c-content--p qx-subtle">{POOL_ABOUT[pool.pool_type]}</p>
					</div>
					<div class="pf-v6-l-split__item">
						<button
							class="pf-v6-c-button pf-m-link pf-m-danger"
							type="button"
							disabled={busy}
							onclick={() => (action = { kind: 'confirming-delete' })}
						>
							<span class="pf-v6-c-button__text"
								>{action.kind === 'running' && action.action === 'delete'
									? 'Deleting…'
									: 'Delete'}</span
							>
						</button>
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
									<table class="pf-v6-c-table pf-m-compact" aria-label="Hosts">
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Host</th>
												<td class="pf-v6-c-table__td"></td>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each hosts as host (host.host_id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td" data-label="Host">
														<a href={resolve('/hosts/[id]', { id: host.host_id })}
															>{host.host_name}</a
														>
													</td>
													<td class="pf-v6-c-table__td pf-v6-c-table__action">
														<button
															class="pf-v6-c-button pf-m-link pf-m-danger pf-m-small"
															type="button"
															disabled={busy}
															aria-label="Detach from {host.host_name}"
															onclick={() => (action = { kind: 'confirming-detach', host })}
														>
															<span class="pf-v6-c-button__text">Detach</span>
														</button>
													</td>
												</tr>
											{/each}
										</tbody>
									</table>
								{:else}
									<p class="qx-subtle">No host can use it yet.</p>
								{/if}
								{#if attachable.length}
									<form class="attach" novalidate onsubmit={attachHost}>
										<div class="pf-v6-c-input-group">
											<div class="pf-v6-c-input-group__item pf-m-fill">
												<FormControl kind="select">
													<select bind:value={attachHostId} aria-label="Host to attach">
														<option value="" disabled>Pick a host to attach</option>
														{#each attachable as host (host.id)}
															<option value={host.id}>{host.name}</option>
														{/each}
													</select>
												</FormControl>
											</div>
											<div class="pf-v6-c-input-group__item">
												<button
													class="pf-v6-c-button pf-m-secondary"
													type="submit"
													disabled={busy || !attachHostId}
												>
													<span class="pf-v6-c-button__text"
														>{action.kind === 'running' && action.action === 'attach'
															? 'Attaching…'
															: 'Attach'}</span
													>
												</button>
											</div>
										</div>
									</form>
								{:else if hosts.length === 0}
									<p class="qx-subtle">No host is up to attach it to.</p>
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

		{#if action.kind === 'confirming-delete'}
			<ConfirmModal
				title="Delete {pool.name}?"
				confirm="Delete"
				onconfirm={remove}
				oncancel={() => (action = { kind: 'idle' })}
			>
				qarax forgets the pool. A pool that still holds storage objects can't be deleted: delete
				them first.
			</ConfirmModal>
		{:else if action.kind === 'confirming-detach'}
			{@const host = action.host}
			<ConfirmModal
				title="Detach {pool.name} from {host.host_name}?"
				confirm="Detach"
				onconfirm={() => detachHost(host)}
				oncancel={() => (action = { kind: 'idle' })}
			>
				VMs on {host.host_name} can no longer use what it holds.
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

	.intro {
		margin-bottom: var(--pf-t--global--spacer--sm);
	}

	.wrap {
		overflow-wrap: anywhere;
	}

	.failed {
		margin-top: var(--pf-t--global--spacer--md);
	}

	.message {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.attach {
		margin-top: var(--pf-t--global--spacer--md);
		max-width: 28rem;
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
