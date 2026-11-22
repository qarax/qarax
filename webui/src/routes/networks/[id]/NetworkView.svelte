<script lang="ts">
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { allHosts, allVms, api, call } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { NETWORK_STATUS } from '#lib/status.ts';
	import { ago } from '#lib/format.ts';
	import ConfirmModal from '#lib/components/ConfirmModal.svelte';
	import Fact from '#lib/components/Fact.svelte';
	import FormControl from '#lib/components/FormControl.svelte';
	import FormGroup from '#lib/components/FormGroup.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import StatusLabel from '#lib/components/StatusLabel.svelte';

	let { id }: { id: string } = $props();

	// The page re-creates this view for another network ({#key}), so the id is fixed.
	const networkId = untrack(() => id);
	const path = { params: { path: { network_id: networkId } } };

	const detail = new Remote(async () => {
		const [network, hosts, addresses, vms, allHostList] = await Promise.all([
			call(api.GET('/v1/networks/{network_id}', path)),
			call(api.GET('/v1/networks/{network_id}/hosts', path)),
			call(api.GET('/v1/networks/{network_id}/ips', path)),
			allVms(),
			allHosts()
		]);
		const attached = new Set(hosts.map((host) => host.host_id));
		return {
			network,
			hosts,
			addresses,
			vmNames: new Map(vms.map((vm) => [vm.id, vm.name])),
			// A host is set up for the network as it is attached, so it has to be up.
			attachable: allHostList.filter((host) => host.status === 'up' && !attached.has(host.id))
		};
	});
	// Addresses come and go with VMs; the event stream says when VMs change.
	keepFresh(detail.refresh);

	type Attached = { host_id: string; host_name: string };
	type ActionState =
		| { kind: 'idle' }
		| { kind: 'confirming-delete' }
		| { kind: 'confirming-detach'; host: Attached }
		| { kind: 'running'; action: 'attach' | 'detach' | 'delete' }
		| { kind: 'failed'; message: string };
	let action: ActionState = $state({ kind: 'idle' });

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

	/** Linux names an interface in at most 15 characters, with no slash or space. */
	const bridgeError = (name: string) =>
		!name
			? 'Name the bridge, such as qbr0.'
			: name.length > 15 || /[\s/]/.test(name)
				? 'Use at most 15 characters, without spaces or slashes.'
				: undefined;

	let attach = $state({ hostId: '', bridge: '', parent: '', tried: false });
	const attachErrors = $derived(
		attach.tried
			? {
					host: attach.hostId ? undefined : 'Pick the host to attach it to.',
					bridge: bridgeError(attach.bridge.trim())
				}
			: {}
	);

	async function attachHost(event: SubmitEvent) {
		event.preventDefault();
		attach.tried = true;
		if (attachErrors.host || attachErrors.bridge) return;
		const done = await run('attach', () =>
			call(
				api.POST('/v1/networks/{network_id}/hosts', {
					...path,
					body: {
						host_id: attach.hostId,
						bridge_name: attach.bridge.trim(),
						parent_interface: attach.parent.trim() || null
					}
				})
			)
		);
		void detail.refresh();
		if (done) attach = { hostId: '', bridge: '', parent: '', tried: false };
	}

	async function detachHost(host: Attached) {
		await run('detach', () =>
			call(
				api.DELETE('/v1/networks/{network_id}/hosts/{host_id}', {
					params: { path: { network_id: networkId, host_id: host.host_id } }
				})
			)
		);
		void detail.refresh();
	}

	async function remove() {
		if (await run('delete', () => call(api.DELETE('/v1/networks/{network_id}', path))))
			await goto(resolve('/networks'));
		else void detail.refresh();
	}
</script>

<section class="pf-v6-c-page__main-breadcrumb">
	<div class="pf-v6-c-page__main-body">
		<nav class="pf-v6-c-breadcrumb" aria-label="Breadcrumb">
			<ol class="pf-v6-c-breadcrumb__list" role="list">
				<li class="pf-v6-c-breadcrumb__item">
					<a href={resolve('/networks')} class="pf-v6-c-breadcrumb__link">Networks</a>
				</li>
				<li class="pf-v6-c-breadcrumb__item">
					<span class="pf-v6-c-breadcrumb__item-divider"><Icon name="chevronRight" /></span>
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page">
						{detail.state.kind === 'ready' || detail.state.kind === 'stale'
							? detail.state.data.network.name
							: 'Network'}
					</span>
				</li>
			</ol>
		</nav>
	</div>
</section>

<Loaded remote={detail} what="this network">
	{#snippet children({ network, hosts, addresses, vmNames, attachable })}
		{@const busy = action.kind === 'running'}
		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="pf-v6-l-split pf-m-gutter pf-m-wrap">
					<div class="pf-v6-l-split__item pf-m-fill">
						<div class="title">
							<h1 class="pf-v6-c-title pf-m-2xl">{network.name}</h1>
							<StatusLabel look={NETWORK_STATUS[network.status]} />
						</div>
						<p class="pf-v6-c-content--p qx-subtle">
							VMs on it get an address in {network.subnet}{#if network.vpc_name}, and can reach the
								other networks of the VPC {network.vpc_name}{/if}.
						</p>
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
									<Fact term="Subnet"><span class="qx-mono">{network.subnet}</span></Fact>
									<Fact term="Gateway">
										{#if network.gateway}<span class="qx-mono">{network.gateway}</span>{:else}<span
												class="qx-subtle">None</span
											>{/if}
									</Fact>
									<Fact term="DNS server">
										{#if network.dns}<span class="qx-mono">{network.dns}</span>{:else}<span
												class="qx-subtle">None</span
											>{/if}
									</Fact>
									<Fact term="Type">
										{#if network.type}{network.type}{:else}<span class="qx-subtle">Not set</span
											>{/if}
									</Fact>
									<Fact term="VPC">
										{#if network.vpc_name}{network.vpc_name}{:else}<span class="qx-subtle"
												>None</span
											>{/if}
									</Fact>
									<Fact term="ID"><span class="qx-mono">{network.id}</span></Fact>
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
									<p class="qx-subtle intro">
										VMs on these hosts can join it, through the bridge on each.
									</p>
									<table class="pf-v6-c-table pf-m-compact" aria-label="Hosts">
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Host</th>
												<th class="pf-v6-c-table__th" scope="col">Bridge</th>
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
													<td class="pf-v6-c-table__td qx-mono" data-label="Bridge"
														>{host.bridge_name}</td
													>
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
									<p class="qx-subtle">No host carries it yet, so no VM can join it.</p>
								{/if}
								{#if attachable.length}
									<form class="pf-v6-c-form attach" novalidate onsubmit={attachHost}>
										<h3 class="pf-v6-c-form__section-title">Attach to a host</h3>
										<FormGroup id="attach-host" label="Host" required error={attachErrors.host}>
											<FormControl kind="select" invalid={!!attachErrors.host}>
												<select
													id="attach-host"
													bind:value={attach.hostId}
													aria-invalid={attachErrors.host ? 'true' : undefined}
													aria-describedby={attachErrors.host ? 'attach-host-helper' : undefined}
												>
													<option value="" disabled>Pick a host</option>
													{#each attachable as host (host.id)}
														<option value={host.id}>{host.name}</option>
													{/each}
												</select>
											</FormControl>
										</FormGroup>
										<FormGroup
											id="attach-bridge"
											label="Bridge"
											required
											error={attachErrors.bridge}
										>
											<FormControl invalid={!!attachErrors.bridge}>
												<input
													type="text"
													id="attach-bridge"
													class="qx-mono"
													autocomplete="off"
													spellcheck="false"
													placeholder="qbr0"
													bind:value={attach.bridge}
													aria-invalid={attachErrors.bridge ? 'true' : undefined}
													aria-describedby="attach-bridge-helper"
												/>
											</FormControl>
											{#snippet helper()}
												The bridge device qarax creates on the host for it.
											{/snippet}
										</FormGroup>
										<FormGroup id="attach-parent" label="Parent interface">
											<FormControl>
												<input
													type="text"
													id="attach-parent"
													class="qx-mono"
													autocomplete="off"
													spellcheck="false"
													placeholder="eth0"
													bind:value={attach.parent}
													aria-describedby="attach-parent-helper"
												/>
											</FormControl>
											{#snippet helper()}
												A host NIC to put on the bridge, so VMs share that NIC's network without
												NAT. Leave it empty for a bridge of its own, with NAT out of the host.
											{/snippet}
										</FormGroup>
										<div class="pf-v6-c-form__group pf-m-action">
											<div class="pf-v6-c-form__group-control">
												<button class="pf-v6-c-button pf-m-secondary" type="submit" disabled={busy}>
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
								<h2 class="pf-v6-c-card__title-text">Addresses in use</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if addresses.length === 0}
									<p class="qx-subtle">
										No VM has an address on it yet. A VM created on this network gets one.
									</p>
								{:else}
									<table class="pf-v6-c-table pf-m-compact pf-m-grid-md" aria-label="Addresses">
										<thead class="pf-v6-c-table__thead">
											<tr class="pf-v6-c-table__tr">
												<th class="pf-v6-c-table__th" scope="col">Address</th>
												<th class="pf-v6-c-table__th" scope="col">VM</th>
												<th class="pf-v6-c-table__th" scope="col">Given</th>
											</tr>
										</thead>
										<tbody class="pf-v6-c-table__tbody">
											{#each addresses as address (address.id)}
												<tr class="pf-v6-c-table__tr">
													<td class="pf-v6-c-table__td qx-mono" data-label="Address"
														>{address.ip_address}</td
													>
													<td class="pf-v6-c-table__td" data-label="VM">
														{#if address.vm_id}
															<a href={resolve('/vms/[id]', { id: address.vm_id })}
																>{vmNames.get(address.vm_id) ?? address.vm_id}</a
															>
														{:else}
															<span class="qx-subtle">Reserved, not on a VM</span>
														{/if}
													</td>
													<td class="pf-v6-c-table__td" data-label="Given">
														<time datetime={address.allocated_at}>{ago(address.allocated_at)}</time>
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
			</div>
		</section>

		{#if action.kind === 'confirming-delete'}
			<ConfirmModal
				title="Delete {network.name}?"
				confirm="Delete"
				onconfirm={remove}
				oncancel={() => (action = { kind: 'idle' })}
			>
				qarax tears it down on each host it is attached to (its bridge, DHCP and NAT) and forgets
				it. A network that VMs or VM templates use can't be deleted.
			</ConfirmModal>
		{:else if action.kind === 'confirming-detach'}
			{@const host = action.host}
			<ConfirmModal
				title="Detach {network.name} from {host.host_name}?"
				confirm="Detach"
				onconfirm={() => detachHost(host)}
				oncancel={() => (action = { kind: 'idle' })}
			>
				qarax removes its bridge, DHCP and NAT from {host.host_name}, so VMs there lose the network.
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

	.failed {
		margin-top: var(--pf-t--global--spacer--md);
	}

	.message {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.attach {
		margin-top: var(--pf-t--global--spacer--lg);
	}
</style>
