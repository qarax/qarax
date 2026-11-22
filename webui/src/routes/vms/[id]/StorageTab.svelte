<script lang="ts">
	import { resolve } from '$app/paths';
	import { allStoragePools, api, call, type Snapshot, type Vm } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { SNAPSHOT_STATUS } from '#lib/status.ts';
	import { OBJECT_TYPE, ago, bytes } from '#lib/format.ts';
	import ConfirmModal from '#lib/components/ConfirmModal.svelte';
	import FormControl from '#lib/components/FormControl.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import Status from '#lib/components/Status.svelte';

	/** A VM's disks and snapshots, and what can be done with its snapshots. */
	let { vm }: { vm: Vm } = $props();

	// The VM's page re-creates its view, tabs included, for another VM.
	const vmId = $derived(vm.id);
	const path = () => ({ params: { path: { vm_id: vmId } } });

	const storage = new Remote(async () => {
		const [disks, snapshots, pools] = await Promise.all([
			call(api.GET('/v1/vms/{vm_id}/disks', path())),
			call(api.GET('/v1/vms/{vm_id}/snapshots', path())),
			allStoragePools()
		]);
		const objects = await Promise.all(
			disks.flatMap((disk) =>
				disk.storage_object_id
					? [
							call(
								api.GET('/v1/storage-objects/{object_id}', {
									params: { path: { object_id: disk.storage_object_id } }
								})
							)
						]
					: []
			)
		);
		return {
			disks,
			snapshots: snapshots.toSorted((a, b) => b.created_at.localeCompare(a.created_at)),
			objects: new Map(objects.map((object) => [object.id, object])),
			pools: new Map(pools.map((pool) => [pool.id, pool]))
		};
	});
	keepFresh(storage.refresh, (event) => event.vm_id === vmId);

	/**
	 * A snapshot is taken of a running VM: qarax pauses it for the moment it
	 * takes. Restoring boots the VM from one, from the statuses a start is
	 * allowed from (`VmOp::Restore` in qarax/src/model/vms.rs).
	 */
	const canTake = $derived(vm.status === 'running');
	const canRestore = $derived(['created', 'shutdown', 'unknown'].includes(vm.status));

	type Work = 'take' | 'restore' | 'delete';
	type Action =
		| { kind: 'idle' }
		| { kind: 'confirming'; what: 'restore' | 'delete'; snapshot: Snapshot }
		| { kind: 'running'; what: Work; snapshotId: string | null }
		| { kind: 'failed'; message: string };
	let action: Action = $state({ kind: 'idle' });
	let snapshotName = $state('');

	const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

	async function run(what: Work, snapshotId: string | null, request: () => Promise<unknown>) {
		action = { kind: 'running', what, snapshotId };
		try {
			await request();
			action = { kind: 'idle' };
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
		}
		void storage.refresh();
	}

	function take(event: SubmitEvent) {
		event.preventDefault();
		const name = snapshotName.trim() || null;
		void run('take', null, async () => {
			await call(api.POST('/v1/vms/{vm_id}/snapshots', { ...path(), body: { name } }));
			snapshotName = '';
		});
	}

	const restore = (snapshot: Snapshot) =>
		run('restore', snapshot.id, () =>
			call(api.POST('/v1/vms/{vm_id}/restore', { ...path(), body: { snapshot_id: snapshot.id } }))
		);

	const remove = (snapshot: Snapshot) =>
		run('delete', snapshot.id, () =>
			call(
				api.DELETE('/v1/vms/{vm_id}/snapshots/{snapshot_id}', {
					params: { path: { vm_id: vmId, snapshot_id: snapshot.id } }
				})
			)
		);
</script>

<Loaded remote={storage} what="its storage">
	{#snippet children({ disks, snapshots, objects, pools })}
		{@const busy = action.kind === 'running'}
		{@const doing = (what: Work, snapshotId: string | null = null) =>
			action.kind === 'running' && action.what === what && action.snapshotId === snapshotId}
		<div class="pf-v6-l-grid pf-m-gutter">
			<div class="pf-v6-l-grid__item pf-m-12-col">
				<div class="pf-v6-c-card">
					<div class="pf-v6-c-card__title">
						<h2 class="pf-v6-c-card__title-text">Disks</h2>
					</div>
					<div class="pf-v6-c-card__body">
						{#if disks.length === 0}
							<p class="qx-subtle">
								None: it boots from a kernel and initramfs alone. Attach one with <code
									>qarax vm attach-disk</code
								>.
							</p>
						{:else}
							<table class="pf-v6-c-table pf-m-compact pf-m-grid-md" aria-label="Disks">
								<thead class="pf-v6-c-table__thead">
									<tr class="pf-v6-c-table__tr">
										<th class="pf-v6-c-table__th" scope="col">Name</th>
										<th class="pf-v6-c-table__th" scope="col">Storage object</th>
										<th class="pf-v6-c-table__th" scope="col">Type</th>
										<th class="pf-v6-c-table__th" scope="col">Pool</th>
										<th class="pf-v6-c-table__th" scope="col">Size</th>
										<th class="pf-v6-c-table__th" scope="col">Access</th>
									</tr>
								</thead>
								<tbody class="pf-v6-c-table__tbody">
									{#each disks as disk (disk.id)}
										{@const object = disk.storage_object_id
											? objects.get(disk.storage_object_id)
											: undefined}
										{@const pool = object ? pools.get(object.storage_pool_id) : undefined}
										<tr class="pf-v6-c-table__tr">
											<td class="pf-v6-c-table__td qx-mono" data-label="Name">
												{disk.logical_name}
											</td>
											<td class="pf-v6-c-table__td" data-label="Storage object">
												{#if object}{object.name}{:else}<span class="qx-subtle">None</span>{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="Type">
												{#if object}{OBJECT_TYPE[object.object_type]}{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="Pool">
												{#if pool}
													<a href={resolve('/storage/[id]', { id: pool.id })}>{pool.name}</a>
												{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="Size">
												{#if object && object.size_bytes > 0}{bytes(object.size_bytes)}{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="Access">
												{disk.read_only ? 'Read-only' : 'Read-write'}
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
							<p class="qx-subtle more">
								Attach, grow or detach a disk with <code>qarax vm attach-disk</code>,
								<code>qarax vm resize-disk</code> and <code>qarax vm remove-disk</code>.
							</p>
						{/if}
					</div>
				</div>
			</div>

			<div class="pf-v6-l-grid__item pf-m-12-col">
				<div class="pf-v6-c-card">
					<div class="pf-v6-c-card__title">
						<h2 class="pf-v6-c-card__title-text">Snapshots</h2>
					</div>
					<div class="pf-v6-c-card__body">
						<p class="qx-subtle intro">
							A snapshot keeps {vm.name}'s memory and device state, so it can be brought back
							running as it was. Its disks aren't in it.
						</p>
						{#if canTake}
							<form class="take" novalidate onsubmit={take}>
								<div class="pf-v6-c-input-group">
									<div class="pf-v6-c-input-group__item pf-m-fill">
										<FormControl>
											<input
												type="text"
												autocomplete="off"
												spellcheck="false"
												placeholder="Name (optional)"
												aria-label="Snapshot name"
												aria-describedby="take-hint"
												bind:value={snapshotName}
											/>
										</FormControl>
									</div>
									<div class="pf-v6-c-input-group__item">
										<button class="pf-v6-c-button pf-m-secondary" type="submit" disabled={busy}>
											<span class="pf-v6-c-button__text"
												>{doing('take') ? 'Taking…' : 'Take snapshot'}</span
											>
										</button>
									</div>
								</div>
								<p class="qx-subtle hint" id="take-hint">
									{vm.name} is paused while it is taken.
								</p>
							</form>
						{:else}
							<p class="qx-subtle hint">A snapshot is taken of a running VM.</p>
						{/if}

						{#if action.kind === 'failed'}
							<div class="failed">
								<Notice tone="danger" title="That didn't work"
									><span class="message">{action.message}</span></Notice
								>
							</div>
						{/if}

						{#if snapshots.length === 0}
							<p class="qx-subtle">None yet.</p>
						{:else}
							{#if !canRestore}
								<p class="qx-subtle hint">To restore {vm.name} from one, stop it first.</p>
							{/if}
							<table class="pf-v6-c-table pf-m-compact pf-m-grid-md" aria-label="Snapshots">
								<thead class="pf-v6-c-table__thead">
									<tr class="pf-v6-c-table__tr">
										<th class="pf-v6-c-table__th" scope="col">Name</th>
										<th class="pf-v6-c-table__th" scope="col">Status</th>
										<th class="pf-v6-c-table__th" scope="col">Taken</th>
										<td class="pf-v6-c-table__td"></td>
									</tr>
								</thead>
								<tbody class="pf-v6-c-table__tbody">
									{#each snapshots as snapshot (snapshot.id)}
										<tr class="pf-v6-c-table__tr">
											<td class="pf-v6-c-table__td" data-label="Name">{snapshot.name}</td>
											<td class="pf-v6-c-table__td" data-label="Status"
												><Status look={SNAPSHOT_STATUS[snapshot.status]} /></td
											>
											<td class="pf-v6-c-table__td" data-label="Taken">
												<time datetime={snapshot.created_at}>{ago(snapshot.created_at)}</time>
											</td>
											<td class="pf-v6-c-table__td pf-v6-c-table__action">
												<div class="row-actions">
													{#if canRestore && snapshot.status === 'ready'}
														<button
															class="pf-v6-c-button pf-m-secondary pf-m-small"
															type="button"
															disabled={busy}
															aria-label="Restore from {snapshot.name}"
															onclick={() =>
																(action = { kind: 'confirming', what: 'restore', snapshot })}
														>
															<span class="pf-v6-c-button__text"
																>{doing('restore', snapshot.id) ? 'Restoring…' : 'Restore'}</span
															>
														</button>
													{/if}
													{#if snapshot.status !== 'creating'}
														<button
															class="pf-v6-c-button pf-m-link pf-m-danger pf-m-small"
															type="button"
															disabled={busy}
															aria-label="Delete {snapshot.name}"
															onclick={() =>
																(action = { kind: 'confirming', what: 'delete', snapshot })}
														>
															<span class="pf-v6-c-button__text"
																>{doing('delete', snapshot.id) ? 'Deleting…' : 'Delete'}</span
															>
														</button>
													{/if}
												</div>
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

		{#if action.kind === 'confirming'}
			{@const { what, snapshot } = action}
			{#if what === 'restore'}
				<ConfirmModal
					title="Restore {vm.name} from {snapshot.name}?"
					confirm="Restore"
					tone="warning"
					onconfirm={() => restore(snapshot)}
					oncancel={() => (action = { kind: 'idle' })}
				>
					It comes back running, with the memory and devices it had when the snapshot was taken. Its
					disks keep what was written to them since, which its guest may not expect.
				</ConfirmModal>
			{:else}
				<ConfirmModal
					title="Delete {snapshot.name}?"
					confirm="Delete"
					onconfirm={() => remove(snapshot)}
					oncancel={() => (action = { kind: 'idle' })}
				>
					The memory and device state it keeps are deleted with it. This can't be undone.
				</ConfirmModal>
			{/if}
		{/if}
	{/snippet}
</Loaded>

<style>
	.intro {
		margin-bottom: var(--pf-t--global--spacer--md);
	}

	.more {
		margin-top: var(--pf-t--global--spacer--md);
	}

	.take {
		max-width: 36rem;
	}

	.hint {
		margin-block: var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--md);
		font-size: var(--pf-t--global--font--size--body--sm);
	}

	.failed {
		margin-bottom: var(--pf-t--global--spacer--md);
	}

	/* Errors from a node can span lines (a command's output, say). */
	.message {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.row-actions {
		display: flex;
		justify-content: flex-end;
		gap: var(--pf-t--global--spacer--sm);
	}
</style>
