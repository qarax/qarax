<script lang="ts">
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { allHosts, allNetworks, api, call, type Host, type Vm } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { VM_STATUS, VM_STATUS_HELP } from '#lib/status.ts';
	import { HYPERVISOR, bytes } from '#lib/format.ts';
	import {
		ACTION_LABEL,
		ACTION_PROGRESS,
		VM_ACTIONS,
		runVmAction,
		type VmAction
	} from '#lib/vm-actions.ts';
	import Fact from '#lib/components/Fact.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import StatusLabel from '#lib/components/StatusLabel.svelte';

	let { id }: { id: string } = $props();

	// The page re-creates this view for another VM ({#key}), so the id is fixed.
	const vmId = untrack(() => id);
	const path = { params: { path: { vm_id: vmId } } };

	const detail = new Remote(async () => {
		const [vm, nics, hosts, networks] = await Promise.all([
			call(api.GET('/v1/vms/{vm_id}', path)),
			call(api.GET('/v1/vms/{vm_id}/nics', path)),
			allHosts(),
			allNetworks()
		]);
		return {
			vm,
			nics,
			host: hosts.find((host) => host.id === vm.host_id),
			networkNames: new Map(networks.map((network) => [network.id, network.name]))
		};
	});
	keepFresh(detail.refresh, (event) => event.vm_id === vmId);

	const consoleLog = new Remote(() =>
		call(api.GET('/v1/vms/{vm_id}/console', { ...path, parseAs: 'text' }))
	);

	const TABS = [
		{ id: 'details', label: 'Details' },
		{ id: 'console', label: 'Console' },
		{ id: 'log', label: 'Console log' }
	] as const;
	type Tab = (typeof TABS)[number]['id'];
	let tab: Tab = $state('details');

	$effect(() => {
		if (tab === 'log') untrack(() => consoleLog.refresh());
	});

	/** Actions that can't be taken back ask first, saying what they do. */
	type Drastic = 'force-stop' | 'delete';

	type ActionState =
		| { kind: 'idle' }
		| { kind: 'confirming'; action: Drastic }
		| { kind: 'running'; action: VmAction | 'delete' }
		| { kind: 'failed'; message: string };
	let action: ActionState = $state({ kind: 'idle' });

	let dialog: HTMLDialogElement | undefined = $state();
	$effect(() => {
		if (!dialog) return;
		if (action.kind === 'confirming' && !dialog.open) dialog.showModal();
		if (action.kind !== 'confirming' && dialog.open) dialog.close();
	});

	const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

	async function act(name: VmAction) {
		action = { kind: 'running', action: name };
		try {
			const { finished } = await runVmAction(vmId, name);
			// Free the buttons once the request is in, not when it's done: a VM
			// stays pending while its start job runs, and force stop gives up on it.
			action = { kind: 'idle' };
			void detail.refresh();
			await finished;
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
			void detail.refresh();
		}
	}

	async function remove() {
		action = { kind: 'running', action: 'delete' };
		try {
			await call(api.DELETE('/v1/vms/{vm_id}', path));
			await goto(resolve('/vms'));
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
		}
	}

	const cancel = () => (action = { kind: 'idle' });

	/** What the status means here: where it runs, or what the status is. */
	function explain(vm: Vm, host: Host | undefined): string {
		if (vm.status === 'running' && host) return `Running on ${host.name}.`;
		return VM_STATUS_HELP[vm.status];
	}

	const label = (name: VmAction | 'delete') =>
		action.kind === 'running' && action.action === name
			? ACTION_PROGRESS[name]
			: name === 'delete'
				? 'Delete'
				: ACTION_LABEL[name];
</script>

<section class="pf-v6-c-page__main-breadcrumb">
	<div class="pf-v6-c-page__main-body">
		<nav class="pf-v6-c-breadcrumb" aria-label="Breadcrumb">
			<ol class="pf-v6-c-breadcrumb__list" role="list">
				<li class="pf-v6-c-breadcrumb__item">
					<a href={resolve('/vms')} class="pf-v6-c-breadcrumb__link">Virtual machines</a>
				</li>
				<li class="pf-v6-c-breadcrumb__item">
					<span class="pf-v6-c-breadcrumb__item-divider"><Icon name="chevronRight" /></span>
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page">
						{detail.state.kind === 'ready' || detail.state.kind === 'stale'
							? detail.state.data.vm.name
							: 'Virtual machine'}
					</span>
				</li>
			</ol>
		</nav>
	</div>
</section>

<Loaded remote={detail} what="this VM">
	{#snippet children({ vm, nics, host, networkNames })}
		{@const look = VM_STATUS[vm.status]}
		{@const actions = VM_ACTIONS[vm.status].filter((name) => name !== 'force-stop')}
		{@const canForceStop = VM_ACTIONS[vm.status].includes('force-stop')}
		{@const busy = action.kind === 'running'}
		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="pf-v6-l-split pf-m-gutter pf-m-wrap">
					<div class="pf-v6-l-split__item pf-m-fill">
						<div class="title">
							<h1 class="pf-v6-c-title pf-m-2xl">{vm.name}</h1>
							<StatusLabel {look} />
						</div>
						<p class="pf-v6-c-content--p qx-subtle">{explain(vm, host)}</p>
					</div>
					<div class="pf-v6-l-split__item actions">
						{#each actions as name, i (name)}
							<button
								class="pf-v6-c-button {i === 0 ? 'pf-m-primary' : 'pf-m-secondary'}"
								type="button"
								disabled={busy}
								onclick={() => act(name)}
							>
								<span class="pf-v6-c-button__text">{label(name)}</span>
							</button>
						{/each}
						{#if canForceStop}
							<button
								class="pf-v6-c-button pf-m-secondary pf-m-danger"
								type="button"
								disabled={busy}
								onclick={() => (action = { kind: 'confirming', action: 'force-stop' })}
							>
								<span class="pf-v6-c-button__text">{label('force-stop')}</span>
							</button>
						{/if}
						<button
							class="pf-v6-c-button pf-m-link pf-m-danger"
							type="button"
							disabled={busy}
							onclick={() => (action = { kind: 'confirming', action: 'delete' })}
						>
							<span class="pf-v6-c-button__text">{label('delete')}</span>
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

		<section class="pf-v6-c-page__main-tabs">
			<div class="pf-v6-c-page__main-body">
				<div class="pf-v6-c-tabs" role="region" aria-label="About {vm.name}">
					<ul class="pf-v6-c-tabs__list" role="tablist">
						{#each TABS as t (t.id)}
							<li class="pf-v6-c-tabs__item" class:pf-m-current={tab === t.id} role="presentation">
								<button
									type="button"
									class="pf-v6-c-tabs__link"
									role="tab"
									id="tab-{t.id}"
									aria-selected={tab === t.id}
									aria-controls="tabpanel-{t.id}"
									onclick={() => (tab = t.id)}
								>
									<span class="pf-v6-c-tabs__item-text">{t.label}</span>
								</button>
							</li>
						{/each}
					</ul>
				</div>
			</div>
		</section>

		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div role="tabpanel" id="tabpanel-{tab}" aria-labelledby="tab-{tab}">
					{#if tab === 'details'}
						<div class="pf-v6-l-grid pf-m-gutter">
							<div class="pf-v6-l-grid__item pf-m-12-col pf-m-6-col-on-xl">
								<div class="pf-v6-c-card pf-m-full-height">
									<div class="pf-v6-c-card__title">
										<h2 class="pf-v6-c-card__title-text">Machine</h2>
									</div>
									<div class="pf-v6-c-card__body">
										<dl class="pf-v6-c-description-list pf-m-horizontal">
											<Fact term="vCPUs">
												{vm.boot_vcpus}
												{#if vm.max_vcpus > vm.boot_vcpus}<span class="qx-subtle"
														>, can grow to {vm.max_vcpus}</span
													>{/if}
											</Fact>
											<Fact term="Memory">
												{bytes(vm.memory_size)}
												{#if vm.memory_hotplug_size}<span class="qx-subtle"
														>, can grow by {bytes(vm.memory_hotplug_size)}</span
													>{/if}
											</Fact>
											<Fact term="Hypervisor">{HYPERVISOR[vm.hypervisor]}</Fact>
											<Fact term="Boots from">
												{#if vm.image_ref}<span class="qx-mono">{vm.image_ref}</span
													>{:else}{vm.boot_mode === 'firmware' ? 'Firmware' : 'A kernel'}{/if}
											</Fact>
											<Fact term="Guest agent">
												{#if vm.guest_agent}On <span class="qx-subtle"
														>(qarax can run commands in it)</span
													>{:else}Off{/if}
											</Fact>
											<Fact term="High availability">
												{#if vm.ha_enabled}On <span class="qx-subtle"
														>(restarted on another host if its host fails)</span
													>{:else}Off{/if}
											</Fact>
										</dl>
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
											<Fact term="Host">
												{#if host}
													{host.name} <span class="qx-mono qx-subtle">{host.address}</span>
												{:else if vm.host_id}
													<span class="qx-mono">{vm.host_id}</span>
												{:else}
													<span class="qx-subtle">Not placed yet</span>
												{/if}
											</Fact>
											<Fact term="Tags">{vm.tags.length ? vm.tags.join(', ') : 'None'}</Fact>
											{#if vm.description}<Fact term="Description">{vm.description}</Fact>{/if}
											<Fact term="ID"><span class="qx-mono">{vm.id}</span></Fact>
										</dl>
									</div>
								</div>
							</div>
							<div class="pf-v6-l-grid__item pf-m-12-col">
								<div class="pf-v6-c-card">
									<div class="pf-v6-c-card__title">
										<h2 class="pf-v6-c-card__title-text">Network interfaces</h2>
									</div>
									<div class="pf-v6-c-card__body">
										{#if nics.length === 0}
											<p class="qx-subtle">
												None. Add one with <code>qarax vm add-nic</code>, or create the VM with a
												network.
											</p>
										{:else}
											<table
												class="pf-v6-c-table pf-m-compact pf-m-grid-md"
												aria-label="Network interfaces"
											>
												<thead class="pf-v6-c-table__thead">
													<tr class="pf-v6-c-table__tr">
														<th class="pf-v6-c-table__th" scope="col">Device</th>
														<th class="pf-v6-c-table__th" scope="col">MAC address</th>
														<th class="pf-v6-c-table__th" scope="col">IP address</th>
														<th class="pf-v6-c-table__th" scope="col">Network</th>
														<th class="pf-v6-c-table__th" scope="col">Type</th>
													</tr>
												</thead>
												<tbody class="pf-v6-c-table__tbody">
													{#each nics as nic (nic.id)}
														<tr class="pf-v6-c-table__tr">
															<td class="pf-v6-c-table__td qx-mono" data-label="Device"
																>{nic.device_id}</td
															>
															<td class="pf-v6-c-table__td qx-mono" data-label="MAC address"
																>{nic.mac_address ?? ''}</td
															>
															<td class="pf-v6-c-table__td qx-mono" data-label="IP address"
																>{nic.ip_address ?? ''}</td
															>
															<td class="pf-v6-c-table__td" data-label="Network">
																{#if nic.network_id}
																	{networkNames.get(nic.network_id) ?? nic.network_id}
																{:else}
																	<span class="qx-subtle">None</span>
																{/if}
															</td>
															<td class="pf-v6-c-table__td" data-label="Type"
																>{nic.interface_type}</td
															>
														</tr>
													{/each}
												</tbody>
											</table>
										{/if}
									</div>
								</div>
							</div>
						</div>
					{:else if tab === 'console'}
						<div class="pf-v6-c-card">
							<div class="pf-v6-c-card__body">
								{#if vm.status === 'running' || vm.status === 'paused'}
									<!-- xterm.js is most of this page's code; load it for the console only. -->
									{#await import('#lib/components/Console.svelte')}
										<p class="qx-subtle">Loading the console…</p>
									{:then { default: Console }}
										<Console vmId={vm.id} token={session.token} />
									{/await}
								{:else}
									<p class="qx-subtle">
										The console is there while {vm.name} runs. Start it, then come back here to attach.
									</p>
								{/if}
							</div>
						</div>
					{:else}
						<div class="pf-v6-c-card">
							<div class="pf-v6-c-card__header">
								<div class="pf-v6-c-card__actions">
									<button
										class="pf-v6-c-button pf-m-secondary"
										type="button"
										onclick={consoleLog.refresh}
									>
										<span class="pf-v6-c-button__text">Refresh</span>
									</button>
								</div>
								<div class="pf-v6-c-card__title">
									<h2 class="pf-v6-c-card__title-text">Console log</h2>
								</div>
							</div>
							<div class="pf-v6-c-card__body">
								<p class="qx-subtle log-intro">
									What {vm.name} has written to its serial console log.
								</p>
								<Loaded remote={consoleLog} what="the console log">
									{#snippet children(text)}
										<div class="pf-v6-c-code-block">
											<div class="pf-v6-c-code-block__content">
												<pre class="pf-v6-c-code-block__pre log"><code
														class="pf-v6-c-code-block__code">{text || 'The log is empty.'}</code
													></pre>
											</div>
										</div>
									{/snippet}
								</Loaded>
							</div>
						</div>
					{/if}
				</div>
			</div>
		</section>

		<dialog class="qx-modal" bind:this={dialog} onclose={cancel} aria-labelledby="confirm-title">
			{#if action.kind === 'confirming'}
				{@const drastic = action.action}
				<div class="pf-v6-c-modal-box pf-m-sm pf-m-danger">
					<div class="pf-v6-c-modal-box__close">
						<button
							class="pf-v6-c-button pf-m-plain"
							type="button"
							aria-label="Close"
							onclick={cancel}
						>
							<span class="pf-v6-c-button__icon"><Icon name="close" /></span>
						</button>
					</div>
					<header class="pf-v6-c-modal-box__header">
						<div class="pf-v6-c-modal-box__header-main">
							<h1 class="pf-v6-c-modal-box__title pf-m-icon pf-m-danger" id="confirm-title">
								<span class="pf-v6-c-modal-box__title-icon"><Icon name="danger" /></span>
								<span class="pf-v6-c-modal-box__title-text">
									{drastic === 'delete' ? `Delete ${vm.name}?` : `Force stop ${vm.name}?`}
								</span>
							</h1>
						</div>
					</header>
					<div class="pf-v6-c-modal-box__body">
						{#if drastic === 'delete'}
							Its snapshots and backups are deleted with it, and so are disks nothing else uses.
							This can't be undone.
						{:else}
							Its guest gets no chance to shut down, like pulling the power cord. Stop lets it shut
							down cleanly.
						{/if}
					</div>
					<footer class="pf-v6-c-modal-box__footer">
						<button
							class="pf-v6-c-button pf-m-danger"
							type="button"
							onclick={() => (drastic === 'delete' ? remove() : act('force-stop'))}
						>
							<span class="pf-v6-c-button__text"
								>{drastic === 'delete' ? 'Delete' : 'Force stop'}</span
							>
						</button>
						<button class="pf-v6-c-button pf-m-link" type="button" onclick={cancel}>
							<span class="pf-v6-c-button__text">Cancel</span>
						</button>
					</footer>
				</div>
			{/if}
		</dialog>
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

	.failed {
		margin-top: var(--pf-t--global--spacer--md);
	}

	/* Errors from a node can span lines (a command's output, say). */
	.message {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.log-intro {
		margin-bottom: var(--pf-t--global--spacer--md);
	}

	.log {
		max-height: 32rem;
		overflow: auto;
		white-space: pre-wrap;
	}
</style>
