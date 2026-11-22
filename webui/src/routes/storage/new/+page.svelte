<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { api, call, type NewStoragePool, type StoragePoolType } from '#lib/api/client.ts';
	import { POOL_ABOUT, POOL_TYPE } from '#lib/format.ts';
	import FormControl from '#lib/components/FormControl.svelte';
	import FormGroup from '#lib/components/FormGroup.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Notice from '#lib/components/Notice.svelte';

	const TYPES: StoragePoolType[] = ['local', 'nfs', 'overlay_bd', 'block'];

	/** Each type keeps its own location fields, so switching type loses nothing typed. */
	let form = $state({
		name: '',
		type: 'local' as StoragePoolType,
		path: '',
		nfsUrl: '',
		registryUrl: '',
		portal: '',
		iqn: '',
		/** GiB; empty for none. */
		capacity: null as number | null
	});

	type Field = 'name' | 'path' | 'nfsUrl' | 'registryUrl' | 'portal' | 'iqn' | 'capacity';
	type Checked =
		| { kind: 'valid'; request: NewStoragePool }
		| { kind: 'invalid'; errors: Partial<Record<Field, string>> };

	const GIB = 1024 ** 3;

	/** The pool's config as qarax-node reads it (qarax/src/model/storage_pools.rs). */
	function location(errors: Partial<Record<Field, string>>): Record<string, string> {
		switch (form.type) {
			case 'local': {
				const path = form.path.trim();
				if (!path.startsWith('/'))
					errors.path = 'Enter an absolute path, such as /var/lib/qarax/images.';
				return { path };
			}
			case 'nfs': {
				const url = form.nfsUrl.trim();
				if (!/^[^:/]+:\/./.test(url))
					errors.nfsUrl = 'Enter the export as server:/path, such as nfs-server:/exports/qarax.';
				return { url };
			}
			case 'overlay_bd': {
				const url = form.registryUrl.trim();
				if (!/^https?:\/\/./.test(url))
					errors.registryUrl = 'Enter the registry URL, such as http://registry:5000.';
				return { url };
			}
			case 'block': {
				const portal = form.portal.trim();
				const iqn = form.iqn.trim();
				if (!portal) errors.portal = 'Enter the target portal.';
				if (!iqn.startsWith('iqn.'))
					errors.iqn = 'Enter the target IQN, such as iqn.2026-01.com.example:storage.';
				return { portal, iqn };
			}
		}
	}

	const checked: Checked = $derived.by(() => {
		const errors: Partial<Record<Field, string>> = {};
		const name = form.name.trim();
		if (!name) errors.name = 'Give the pool a name.';
		const config = location(errors);
		const capacity = form.capacity === null ? null : Math.round(form.capacity * GIB);
		if (capacity !== null && !(capacity > 0))
			errors.capacity = 'Enter a capacity above 0, or leave it empty.';
		if (Object.keys(errors).length) return { kind: 'invalid', errors };
		return {
			kind: 'valid',
			request: { name, pool_type: form.type, config, capacity_bytes: capacity }
		};
	});

	type Submit = { kind: 'editing'; tried: boolean; error: string | null } | { kind: 'creating' };
	let submit: Submit = $state({ kind: 'editing', tried: false, error: null });
	const errors = $derived(
		submit.kind === 'editing' && submit.tried && checked.kind === 'invalid' ? checked.errors : {}
	);
	const FIELD_IDS: Record<Field, string> = {
		name: 'pool-name',
		path: 'pool-path',
		nfsUrl: 'pool-nfs-url',
		registryUrl: 'pool-registry-url',
		portal: 'pool-portal',
		iqn: 'pool-iqn',
		capacity: 'pool-capacity'
	};

	const describedBy = (id: string, error: string | undefined, helper = false) =>
		error || helper ? `${id}-helper` : undefined;

	async function create(event: SubmitEvent) {
		event.preventDefault();
		if (checked.kind === 'invalid') {
			submit = { kind: 'editing', tried: true, error: null };
			await tick();
			const first = (Object.keys(FIELD_IDS) as Field[]).find((field) => field in checked.errors);
			if (first) document.getElementById(FIELD_IDS[first])?.focus();
			return;
		}
		submit = { kind: 'creating' };
		try {
			const id = await call(
				api.POST('/v1/storage-pools', { body: checked.request, parseAs: 'text' })
			);
			await goto(resolve('/storage/[id]', { id }));
		} catch (e) {
			submit = { kind: 'editing', tried: true, error: e instanceof Error ? e.message : String(e) };
		}
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
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page"
						>Create storage pool</span
					>
				</li>
			</ol>
		</nav>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Create a storage pool</h1>
		<p class="pf-v6-c-content--p qx-subtle">
			An NFS, OverlayBD or iSCSI pool is attached to every host that is up as it is created. A local
			pool is attached to none: attach it on its page to the host whose directory it is.
		</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<form class="pf-v6-c-form pf-m-limit-width" novalidate onsubmit={create}>
			<FormGroup id="pool-name" label="Name" required error={errors.name}>
				<FormControl invalid={!!errors.name}>
					<input
						type="text"
						id="pool-name"
						required
						autocomplete="off"
						spellcheck="false"
						bind:value={form.name}
						aria-invalid={errors.name ? 'true' : undefined}
						aria-describedby={describedBy('pool-name', errors.name)}
					/>
				</FormControl>
			</FormGroup>
			<FormGroup id="pool-type" label="Type" choices="radio">
				{#each TYPES as type (type)}
					<div class="pf-v6-c-radio">
						<input
							class="pf-v6-c-radio__input"
							type="radio"
							id="pool-type-{type}"
							value={type}
							bind:group={form.type}
							aria-describedby="pool-type-{type}-description"
						/>
						<label class="pf-v6-c-radio__label" for="pool-type-{type}">{POOL_TYPE[type]}</label>
						<span class="pf-v6-c-radio__description" id="pool-type-{type}-description"
							>{POOL_ABOUT[type]}</span
						>
					</div>
				{/each}
			</FormGroup>

			{#if form.type === 'local'}
				<FormGroup id="pool-path" label="Directory" required error={errors.path}>
					<FormControl invalid={!!errors.path}>
						<input
							type="text"
							id="pool-path"
							class="qx-mono"
							required
							autocomplete="off"
							spellcheck="false"
							placeholder="/var/lib/qarax/images"
							bind:value={form.path}
							aria-invalid={errors.path ? 'true' : undefined}
							aria-describedby="pool-path-helper"
						/>
					</FormControl>
					{#snippet helper()}
						Its path on each host it is attached to. qarax creates it if it isn't there.
					{/snippet}
				</FormGroup>
			{:else if form.type === 'nfs'}
				<FormGroup id="pool-nfs-url" label="Export" required error={errors.nfsUrl}>
					<FormControl invalid={!!errors.nfsUrl}>
						<input
							type="text"
							id="pool-nfs-url"
							class="qx-mono"
							required
							autocomplete="off"
							spellcheck="false"
							placeholder="nfs-server:/exports/qarax"
							bind:value={form.nfsUrl}
							aria-invalid={errors.nfsUrl ? 'true' : undefined}
							aria-describedby={describedBy('pool-nfs-url', errors.nfsUrl)}
						/>
					</FormControl>
				</FormGroup>
			{:else if form.type === 'overlay_bd'}
				<FormGroup id="pool-registry-url" label="Registry" required error={errors.registryUrl}>
					<FormControl invalid={!!errors.registryUrl}>
						<input
							type="text"
							id="pool-registry-url"
							class="qx-mono"
							required
							autocomplete="off"
							spellcheck="false"
							placeholder="http://registry:5000"
							bind:value={form.registryUrl}
							aria-invalid={errors.registryUrl ? 'true' : undefined}
							aria-describedby="pool-registry-url-helper"
						/>
					</FormControl>
					{#snippet helper()}
						Where qarax keeps the images it converts for lazy loading. Each host needs
						overlaybd-tcmu, which the qarax-node appliance has.
					{/snippet}
				</FormGroup>
			{:else}
				<FormGroup id="pool-portal" label="Target portal" required error={errors.portal}>
					<FormControl invalid={!!errors.portal}>
						<input
							type="text"
							id="pool-portal"
							class="qx-mono"
							required
							autocomplete="off"
							spellcheck="false"
							placeholder="192.0.2.10:3260"
							bind:value={form.portal}
							aria-invalid={errors.portal ? 'true' : undefined}
							aria-describedby="pool-portal-helper"
						/>
					</FormControl>
					{#snippet helper()}
						An IP address, with a port if it isn't 3260.
					{/snippet}
				</FormGroup>
				<FormGroup id="pool-iqn" label="Target IQN" required error={errors.iqn}>
					<FormControl invalid={!!errors.iqn}>
						<input
							type="text"
							id="pool-iqn"
							class="qx-mono"
							required
							autocomplete="off"
							spellcheck="false"
							placeholder="iqn.2026-01.com.example:storage"
							bind:value={form.iqn}
							aria-invalid={errors.iqn ? 'true' : undefined}
							aria-describedby={describedBy('pool-iqn', errors.iqn)}
						/>
					</FormControl>
				</FormGroup>
			{/if}

			<FormGroup id="pool-capacity" label="Capacity" error={errors.capacity}>
				<div class="pf-v6-c-input-group">
					<div class="pf-v6-c-input-group__item pf-m-fill">
						<FormControl invalid={!!errors.capacity}>
							<input
								type="number"
								id="pool-capacity"
								min="0"
								step="any"
								bind:value={form.capacity}
								aria-invalid={errors.capacity ? 'true' : undefined}
								aria-describedby="pool-capacity-helper"
							/>
						</FormControl>
					</div>
					<div class="pf-v6-c-input-group__item">
						<span class="pf-v6-c-input-group__text">GiB</span>
					</div>
				</div>
				{#snippet helper()}
					qarax counts what it allocates in the pool against this, and refuses what doesn't fit.
					Leave it empty for no limit.
				{/snippet}
			</FormGroup>

			{#if submit.kind === 'editing' && submit.error}
				<Notice tone="danger" title="Couldn't create the storage pool">
					<span class="message">{submit.error}</span>
				</Notice>
			{/if}

			<div class="pf-v6-c-form__group pf-m-action">
				<div class="pf-v6-c-form__group-control">
					<div class="pf-v6-c-action-list">
						<div class="pf-v6-c-action-list__group">
							<div class="pf-v6-c-action-list__item">
								<button
									class="pf-v6-c-button pf-m-primary"
									type="submit"
									disabled={submit.kind === 'creating'}
								>
									<span class="pf-v6-c-button__text"
										>{submit.kind === 'creating' ? 'Creating…' : 'Create'}</span
									>
								</button>
							</div>
							<div class="pf-v6-c-action-list__item">
								<a class="pf-v6-c-button pf-m-link" href={resolve('/storage')}>
									<span class="pf-v6-c-button__text">Cancel</span>
								</a>
							</div>
						</div>
					</div>
				</div>
			</div>
		</form>
	</div>
</section>

<style>
	.message {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
</style>
