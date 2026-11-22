<script lang="ts">
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import {
		ApiError,
		allBootSources,
		allNetworks,
		allStorageObjects,
		api,
		call
	} from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { HYPERVISOR, bytes, capitalize } from '#lib/format.ts';
	import { templateBoots } from '#lib/templates.ts';
	import ConfirmModal from '#lib/components/ConfirmModal.svelte';
	import Fact from '#lib/components/Fact.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';

	let { id }: { id: string } = $props();

	// The page re-creates this view for another template ({#key}), so the id is fixed.
	const templateId = untrack(() => id);
	const path = { params: { path: { vm_template_id: templateId } } };

	const detail = new Remote(async () => {
		const [template, pool, bootSources, disks, networks] = await Promise.all([
			call(api.GET('/v1/vm-templates/{vm_template_id}', path)),
			// A template without a sandbox pool is the usual case, which the API answers with 404.
			call(api.GET('/v1/vm-templates/{vm_template_id}/sandbox-pool', path)).catch((e) => {
				if (e instanceof ApiError && e.status === 404) return null;
				throw e;
			}),
			allBootSources(),
			allStorageObjects({ objectType: 'disk' }),
			allNetworks()
		]);
		return {
			template,
			pool,
			names: {
				bootSources: new Map(bootSources.map((source) => [source.id, source.name])),
				disks: new Map(disks.map((disk) => [disk.id, disk.name]))
			},
			networkNames: new Map(networks.map((network) => [network.id, network.name]))
		};
	});
	keepFresh(detail.refresh);

	type ActionState =
		| { kind: 'idle' }
		| { kind: 'confirming-delete' }
		| { kind: 'deleting' }
		| { kind: 'failed'; message: string };
	let action: ActionState = $state({ kind: 'idle' });

	async function remove() {
		action = { kind: 'deleting' };
		try {
			await call(api.DELETE('/v1/vm-templates/{vm_template_id}', path));
			await goto(resolve('/templates'));
		} catch (e) {
			action = { kind: 'failed', message: e instanceof Error ? e.message : String(e) };
		}
	}
</script>

<section class="pf-v6-c-page__main-breadcrumb">
	<div class="pf-v6-c-page__main-body">
		<nav class="pf-v6-c-breadcrumb" aria-label="Breadcrumb">
			<ol class="pf-v6-c-breadcrumb__list" role="list">
				<li class="pf-v6-c-breadcrumb__item">
					<a href={resolve('/templates')} class="pf-v6-c-breadcrumb__link">VM templates</a>
				</li>
				<li class="pf-v6-c-breadcrumb__item">
					<span class="pf-v6-c-breadcrumb__item-divider"><Icon name="chevronRight" /></span>
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page">
						{'data' in detail.state ? detail.state.data.template.name : 'VM template'}
					</span>
				</li>
			</ol>
		</nav>
	</div>
</section>

<Loaded remote={detail} what="this VM template">
	{#snippet children({ template, pool, names, networkNames })}
		<section class="pf-v6-c-page__main-section">
			<div class="pf-v6-c-page__main-body">
				<div class="pf-v6-l-split pf-m-gutter pf-m-wrap">
					<div class="pf-v6-l-split__item pf-m-fill">
						<h1 class="pf-v6-c-title pf-m-2xl title">{template.name}</h1>
						<p class="pf-v6-c-content--p qx-subtle">
							{template.description ??
								'VMs and sandboxes made from it boot and are sized as it says.'}
						</p>
					</div>
					<div class="pf-v6-l-split__item actions">
						<a
							class="pf-v6-c-button pf-m-primary"
							href="{resolve('/vms/new')}?template={template.id}"
						>
							<span class="pf-v6-c-button__text">Create VM</span>
						</a>
						<a
							class="pf-v6-c-button pf-m-secondary"
							href="{resolve('/sandboxes/new')}?template={template.id}"
						>
							<span class="pf-v6-c-button__text">Create sandbox</span>
						</a>
						<button
							class="pf-v6-c-button pf-m-link pf-m-danger"
							type="button"
							disabled={action.kind === 'deleting'}
							onclick={() => (action = { kind: 'confirming-delete' })}
						>
							<span class="pf-v6-c-button__text"
								>{action.kind === 'deleting' ? 'Deleting…' : 'Delete'}</span
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
					<div class="pf-v6-l-grid__item pf-m-12-col pf-m-7-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">What it makes</h2>
							</div>
							<div class="pf-v6-c-card__body">
								<dl class="pf-v6-c-description-list pf-m-horizontal">
									<Fact term="Boots">
										<span class="wrap">{capitalize(templateBoots(template, names))}</span>
									</Fact>
									<Fact term="Hypervisor">
										{#if template.hypervisor}{HYPERVISOR[template.hypervisor]}{:else}<span
												class="qx-subtle"
												>Not set: a VM picks one, a sandbox runs on Firecracker</span
											>{/if}
									</Fact>
									<Fact term="vCPUs">
										{#if template.boot_vcpus}{template.boot_vcpus}{#if template.max_vcpus && template.max_vcpus > template.boot_vcpus}<span
													class="qx-subtle">, can grow to {template.max_vcpus}</span
												>{/if}{:else}<span class="qx-subtle">Not set</span>{/if}
									</Fact>
									<Fact term="Memory">
										{#if template.memory_size}{bytes(template.memory_size)}{:else}<span
												class="qx-subtle">Not set</span
											>{/if}
									</Fact>
									<Fact term="Network">
										{#if template.network_id}
											<a href={resolve('/networks/[id]', { id: template.network_id })}
												>{networkNames.get(template.network_id) ?? template.network_id}</a
											>
										{:else}
											<span class="qx-subtle">None</span>
										{/if}
									</Fact>
									<Fact term="Cloud-init">
										{#if template.cloud_init_user_data}User data{:else}<span class="qx-subtle"
												>None</span
											>{/if}
									</Fact>
									<Fact term="ID"><span class="qx-mono">{template.id}</span></Fact>
								</dl>
							</div>
						</div>
					</div>
					<div class="pf-v6-l-grid__item pf-m-12-col pf-m-5-col-on-xl">
						<div class="pf-v6-c-card pf-m-full-height">
							<div class="pf-v6-c-card__title">
								<h2 class="pf-v6-c-card__title-text">Sandbox pool</h2>
							</div>
							<div class="pf-v6-c-card__body">
								{#if pool}
									<dl class="pf-v6-c-description-list pf-m-horizontal">
										<Fact term="Ready">
											{pool.current_ready}
											<span class="qx-subtle">of {pool.min_ready} kept ready</span>
										</Fact>
										<Fact term="Provisioning">{pool.current_provisioning}</Fact>
										<Fact term="Failed">{pool.current_error}</Fact>
										<Fact term="Makes them by">
											{#if pool.fork}
												Forking {pool.seed_hosts === 1
													? 'a seed'
													: `seeds on ${pool.seed_hosts} hosts`}
											{:else}
												Booting each one
											{/if}
										</Fact>
									</dl>
								{:else}
									<p class="qx-subtle">
										None. A pool keeps sandboxes of it booted ahead of time, so a new one is ready
										the moment it is asked for: <code
											>qarax sandbox pool set --template {template.name} --min-ready 2</code
										>.
									</p>
								{/if}
							</div>
						</div>
					</div>
				</div>
			</div>
		</section>

		{#if action.kind === 'confirming-delete'}
			<ConfirmModal
				title="Delete {template.name}?"
				confirm="Delete"
				onconfirm={remove}
				oncancel={() => (action = { kind: 'idle' })}
			>
				VMs made from it keep what they took from it. A template with a sandbox pool can't be
				deleted: remove its pool first, with <code>qarax sandbox pool delete</code>.
			</ConfirmModal>
		{/if}
	{/snippet}
</Loaded>

<style>
	.title {
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

	.message {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.wrap {
		overflow-wrap: anywhere;
	}
</style>
