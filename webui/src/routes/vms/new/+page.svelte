<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import {
		allBootSources,
		allHosts,
		allInstanceTypes,
		allNetworks,
		allSecurityGroups,
		allStorageObjects,
		allStoragePools,
		api,
		call
	} from '#lib/api/client.ts';
	import { Remote } from '#lib/remote.svelte.ts';
	import { bytes } from '#lib/format.ts';
	import { newVmForm, toNewVm, type Field } from '#lib/vm-create.ts';
	import FormControl from '#lib/components/FormControl.svelte';
	import FormGroup from '#lib/components/FormGroup.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';

	// What the form offers to pick from. Loaded once: choices that changed
	// under someone filling in the form would be worse than ones a minute old.
	const choices = new Remote(async () => {
		const [instanceTypes, bootSources, disks, networks, securityGroups, pools, hosts] =
			await Promise.all([
				allInstanceTypes(),
				allBootSources(),
				allStorageObjects({ objectType: 'disk' }),
				allNetworks(),
				allSecurityGroups(),
				allStoragePools(),
				allHosts()
			]);
		return {
			instanceTypes,
			bootSources,
			disks,
			networks,
			securityGroups,
			hasOverlayBd: pools.some((pool) => pool.pool_type === 'overlay_bd'),
			hostsUp: hosts.filter((host) => host.status === 'up').length
		};
	});
	onMount(choices.refresh);

	let form = $state(newVmForm());
	const checked = $derived(toNewVm(form));

	/** Errors show once a create was tried, and then follow each change. */
	type Submit = { kind: 'editing'; tried: boolean; error: string | null } | { kind: 'creating' };
	let submit: Submit = $state({ kind: 'editing', tried: false, error: null });
	const errors = $derived(
		submit.kind === 'editing' && submit.tried && checked.kind === 'invalid' ? checked.errors : {}
	);

	/** The control each error is about, in the order they appear. */
	const FIELD_IDS: Record<Field, () => string> = {
		name: () => 'vm-name',
		imageRef: () => 'vm-image-ref',
		diskId: () => 'vm-disk',
		hypervisor: () => `vm-hypervisor-${form.hypervisor}`,
		instanceTypeId: () => 'vm-instance-type',
		vcpus: () => 'vm-vcpus',
		memory: () => 'vm-memory'
	};

	const describedBy = (id: string, error: string | undefined, helper = false) =>
		error || helper ? `${id}-helper` : undefined;

	async function create(event: SubmitEvent) {
		event.preventDefault();
		if (checked.kind === 'invalid') {
			submit = { kind: 'editing', tried: true, error: null };
			await tick();
			const first = (Object.keys(FIELD_IDS) as Field[]).find((field) => field in checked.errors);
			if (first) document.getElementById(FIELD_IDS[first]())?.focus();
			return;
		}
		submit = { kind: 'creating' };
		try {
			const created = await call(api.POST('/v1/vms', { body: checked.request }));
			// A VM that boots from an image is created by a job that prepares the
			// image (202); its page follows that job. Any other is there now (201).
			if (typeof created === 'string') {
				await goto(resolve('/vms/[id]', { id: created }));
			} else {
				await goto(`${resolve('/vms/[id]', { id: created.vm_id })}?job=${created.job_id}`);
			}
		} catch (e) {
			submit = { kind: 'editing', tried: true, error: e instanceof Error ? e.message : String(e) };
		}
	}

	const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
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
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page">Create VM</span>
				</li>
			</ol>
		</nav>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Create a virtual machine</h1>
		<p class="pf-v6-c-content--p qx-subtle">
			qarax places it on a host with room for it, and leaves it stopped: start it from its page.
		</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={choices} what="what a VM can use">
			{#snippet children({
				instanceTypes,
				bootSources,
				disks,
				networks,
				securityGroups,
				hasOverlayBd,
				hostsUp
			})}
				{#snippet noDisks()}
					There are no disks yet. Make one from a cloud image with <code
						>qarax storage-pool create-disk --pool &lt;pool&gt; --name &lt;disk&gt; --source
						&lt;image URL&gt;</code
					>.
				{/snippet}
				{#if hostsUp === 0}
					<div class="notice">
						<Notice tone="warning" title="No host is up">
							qarax places a VM on a host that is up when it creates it, and none is right now. <a
								href={resolve('/hosts')}>See the hosts</a
							>
						</Notice>
					</div>
				{/if}
				<form class="pf-v6-c-form pf-m-limit-width" novalidate onsubmit={create}>
					<section class="pf-v6-c-form__section" role="group">
						<FormGroup id="vm-name" label="Name" required error={errors.name}>
							<FormControl invalid={!!errors.name}>
								<input
									type="text"
									id="vm-name"
									required
									autocomplete="off"
									spellcheck="false"
									bind:value={form.name}
									aria-invalid={errors.name ? 'true' : undefined}
									aria-describedby={describedBy('vm-name', errors.name)}
								/>
							</FormControl>
						</FormGroup>
						<FormGroup id="vm-description" label="Description">
							<FormControl>
								<input type="text" id="vm-description" bind:value={form.description} />
							</FormControl>
						</FormGroup>
						<FormGroup id="vm-tags" label="Tags">
							<FormControl>
								<input
									type="text"
									id="vm-tags"
									autocomplete="off"
									spellcheck="false"
									placeholder="web, production"
									bind:value={form.tags}
									aria-describedby="vm-tags-helper"
								/>
							</FormControl>
							{#snippet helper()}
								Separate tags with commas. Lists filter by them, and placement policies can use
								them.
							{/snippet}
						</FormGroup>
					</section>

					<section class="pf-v6-c-form__section" role="group" aria-labelledby="vm-boot-title">
						<h2 class="pf-v6-c-form__section-title" id="vm-boot-title">Boot</h2>
						<FormGroup id="vm-boot" label="Boots from" choices="radio">
							<div class="pf-v6-c-radio">
								<input
									class="pf-v6-c-radio__input"
									type="radio"
									id="vm-boot-image"
									value="image"
									bind:group={form.boot}
									aria-describedby="vm-boot-image-description"
								/>
								<label class="pf-v6-c-radio__label" for="vm-boot-image">An OCI image</label>
								<span class="pf-v6-c-radio__description" id="vm-boot-image-description">
									A container image, such as one from Docker Hub. It boots before the whole image is
									downloaded: the host reads its blocks as the guest needs them.
								</span>
							</div>
							<div class="pf-v6-c-radio">
								<input
									class="pf-v6-c-radio__input"
									type="radio"
									id="vm-boot-kernel"
									value="kernel"
									bind:group={form.boot}
									aria-describedby="vm-boot-kernel-description"
								/>
								<label class="pf-v6-c-radio__label" for="vm-boot-kernel">A kernel</label>
								<span class="pf-v6-c-radio__description" id="vm-boot-kernel-description">
									Linux booted straight from a kernel and initramfs.
								</span>
							</div>
							<div class="pf-v6-c-radio">
								<input
									class="pf-v6-c-radio__input"
									type="radio"
									id="vm-boot-disk"
									value="disk"
									bind:group={form.boot}
									aria-describedby="vm-boot-disk-description"
								/>
								<label class="pf-v6-c-radio__label" for="vm-boot-disk">A disk</label>
								<span class="pf-v6-c-radio__description" id="vm-boot-disk-description">
									A disk with its own bootloader, such as a cloud image, booted through UEFI
									firmware.
								</span>
							</div>
						</FormGroup>

						{#if form.boot === 'image'}
							<FormGroup id="vm-image-ref" label="Image" required error={errors.imageRef}>
								<FormControl invalid={!!errors.imageRef}>
									<input
										type="text"
										id="vm-image-ref"
										required
										autocomplete="off"
										spellcheck="false"
										placeholder="docker.io/library/alpine:latest"
										bind:value={form.imageRef}
										aria-invalid={errors.imageRef ? 'true' : undefined}
										aria-describedby={describedBy('vm-image-ref', errors.imageRef, true)}
									/>
								</FormControl>
								{#snippet helper()}
									{#if hasOverlayBd}
										qarax prepares the image on the VM's host before the VM can start, which can
										take a minute for a large one.
									{:else}
										Booting an image needs an OverlayBD storage pool on the VM's host, and there is
										none yet. Create one with <code
											>qarax storage-pool create --name overlaybd --pool-type overlaybd --url
											&lt;registry&gt;</code
										>, then <code>qarax storage-pool attach-host overlaybd --all</code>.
									{/if}
								{/snippet}
							</FormGroup>
						{:else if form.boot === 'kernel'}
							<FormGroup id="vm-boot-source" label="Boot source">
								<FormControl kind="select">
									<select
										id="vm-boot-source"
										bind:value={form.bootSourceId}
										aria-describedby="vm-boot-source-helper"
									>
										<option value="">The default kernel</option>
										{#each bootSources as source (source.id)}
											<option value={source.id}>{source.name}</option>
										{/each}
									</select>
								</FormControl>
								{#snippet helper()}
									A boot source is a kernel, an optional initramfs and kernel parameters. The
									default is the kernel the control plane is configured with (<code
										>vm_defaults</code
									>).
								{/snippet}
							</FormGroup>
						{:else}
							<FormGroup
								id="vm-disk"
								label="Disk"
								required
								error={errors.diskId}
								helper={disks.length ? undefined : noDisks}
							>
								<FormControl kind="select" invalid={!!errors.diskId}>
									<select
										id="vm-disk"
										required
										bind:value={form.diskId}
										aria-invalid={errors.diskId ? 'true' : undefined}
										aria-describedby={describedBy('vm-disk', errors.diskId, disks.length === 0)}
									>
										<option value="" disabled>
											{disks.length ? 'Pick a disk' : 'No disks yet'}
										</option>
										{#each disks as disk (disk.id)}
											<option value={disk.id}>{disk.name} ({bytes(disk.size_bytes)})</option>
										{/each}
									</select>
								</FormControl>
							</FormGroup>
						{/if}

						<FormGroup
							id="vm-hypervisor"
							label="Hypervisor"
							choices="radio"
							error={errors.hypervisor}
						>
							<div class="pf-v6-c-radio">
								<input
									class="pf-v6-c-radio__input"
									type="radio"
									id="vm-hypervisor-cloud_hv"
									value="cloud_hv"
									bind:group={form.hypervisor}
									aria-describedby="vm-hypervisor-cloud_hv-description"
								/>
								<label class="pf-v6-c-radio__label" for="vm-hypervisor-cloud_hv"
									>Cloud Hypervisor</label
								>
								<span class="pf-v6-c-radio__description" id="vm-hypervisor-cloud_hv-description">
									Boots kernels, and disks through UEFI firmware.
								</span>
							</div>
							<div class="pf-v6-c-radio">
								<input
									class="pf-v6-c-radio__input"
									type="radio"
									id="vm-hypervisor-firecracker"
									value="firecracker"
									bind:group={form.hypervisor}
									aria-describedby={errors.hypervisor
										? 'vm-hypervisor-helper'
										: 'vm-hypervisor-firecracker-description'}
								/>
								<label class="pf-v6-c-radio__label" for="vm-hypervisor-firecracker"
									>Firecracker</label
								>
								<span class="pf-v6-c-radio__description" id="vm-hypervisor-firecracker-description">
									A minimal microVM for kernels, quick to boot and light on memory.
								</span>
							</div>
						</FormGroup>
					</section>

					<section class="pf-v6-c-form__section" role="group" aria-labelledby="vm-size-title">
						<h2 class="pf-v6-c-form__section-title" id="vm-size-title">Size</h2>
						{#if instanceTypes.length}
							<FormGroup id="vm-size" label="Size from" choices="radio">
								<div class="pf-v6-c-radio">
									<input
										class="pf-v6-c-radio__input"
										type="radio"
										id="vm-size-custom"
										value="custom"
										bind:group={form.size}
									/>
									<label class="pf-v6-c-radio__label" for="vm-size-custom">vCPUs and memory</label>
								</div>
								<div class="pf-v6-c-radio">
									<input
										class="pf-v6-c-radio__input"
										type="radio"
										id="vm-size-instance-type"
										value="instance-type"
										bind:group={form.size}
										aria-describedby="vm-size-instance-type-description"
									/>
									<label class="pf-v6-c-radio__label" for="vm-size-instance-type"
										>An instance type</label
									>
									<span class="pf-v6-c-radio__description" id="vm-size-instance-type-description">
										A named size your fleet defines, so VMs of a kind are sized alike.
									</span>
								</div>
							</FormGroup>
						{/if}
						{#if form.size === 'instance-type'}
							<FormGroup
								id="vm-instance-type"
								label="Instance type"
								required
								error={errors.instanceTypeId}
							>
								<FormControl kind="select" invalid={!!errors.instanceTypeId}>
									<select
										id="vm-instance-type"
										required
										bind:value={form.instanceTypeId}
										aria-invalid={errors.instanceTypeId ? 'true' : undefined}
										aria-describedby={describedBy('vm-instance-type', errors.instanceTypeId)}
									>
										<option value="" disabled>Pick an instance type</option>
										{#each instanceTypes as type (type.id)}
											<option value={type.id}>
												{type.name} ({plural(type.boot_vcpus, 'vCPU', 'vCPUs')}, {bytes(
													type.memory_size
												)})
											</option>
										{/each}
									</select>
								</FormControl>
							</FormGroup>
						{:else}
							<FormGroup id="vm-vcpus" label="vCPUs" required error={errors.vcpus}>
								<FormControl invalid={!!errors.vcpus}>
									<input
										type="number"
										id="vm-vcpus"
										required
										min="1"
										step="1"
										bind:value={form.vcpus}
										aria-invalid={errors.vcpus ? 'true' : undefined}
										aria-describedby={describedBy('vm-vcpus', errors.vcpus)}
									/>
								</FormControl>
							</FormGroup>
							<FormGroup id="vm-memory" label="Memory" required error={errors.memory}>
								<div class="pf-v6-c-input-group">
									<div class="pf-v6-c-input-group__item pf-m-fill">
										<FormControl invalid={!!errors.memory}>
											<input
												type="number"
												id="vm-memory"
												required
												min="0"
												step="any"
												bind:value={form.memory}
												aria-invalid={errors.memory ? 'true' : undefined}
												aria-describedby={describedBy('vm-memory', errors.memory)}
											/>
										</FormControl>
									</div>
									<div class="pf-v6-c-input-group__item unit">
										<FormControl kind="select">
											<select bind:value={form.memoryUnit} aria-label="Memory unit">
												<option value="MiB">MiB</option>
												<option value="GiB">GiB</option>
											</select>
										</FormControl>
									</div>
								</div>
							</FormGroup>
						{/if}
					</section>

					<section class="pf-v6-c-form__section" role="group" aria-labelledby="vm-network-title">
						<h2 class="pf-v6-c-form__section-title" id="vm-network-title">Network</h2>
						<FormGroup id="vm-network" label="Network">
							<FormControl kind="select">
								<select
									id="vm-network"
									bind:value={form.networkId}
									aria-describedby="vm-network-helper"
								>
									<option value="">No network</option>
									{#each networks as network (network.id)}
										<option value={network.id}>{network.name} ({network.subnet})</option>
									{/each}
								</select>
							</FormControl>
							{#snippet helper()}
								{#if networks.length === 0}
									There are no networks yet. Create one with <code
										>qarax network create --name &lt;name&gt; --subnet 10.0.0.0/24</code
									>.
								{:else}
									It gets a network interface on it, with an address qarax hands out from its
									subnet.
								{/if}
							{/snippet}
						</FormGroup>
						{#if form.networkId && securityGroups.length}
							<FormGroup id="vm-security-groups" label="Security groups" choices="checkbox">
								{#each securityGroups as group (group.id)}
									<div class="pf-v6-c-check">
										<input
											class="pf-v6-c-check__input"
											type="checkbox"
											id="vm-security-group-{group.id}"
											value={group.id}
											bind:group={form.securityGroupIds}
											aria-describedby={group.description
												? `vm-security-group-${group.id}-description`
												: undefined}
										/>
										<label class="pf-v6-c-check__label" for="vm-security-group-{group.id}"
											>{group.name}</label
										>
										{#if group.description}
											<span
												class="pf-v6-c-check__description"
												id="vm-security-group-{group.id}-description">{group.description}</span
											>
										{/if}
									</div>
								{/each}
								{#snippet helper()}
									Their rules decide what traffic reaches it on the network and what it may send.
								{/snippet}
							</FormGroup>
						{/if}
					</section>

					<section class="pf-v6-c-form__section" role="group" aria-labelledby="vm-options-title">
						<h2 class="pf-v6-c-form__section-title" id="vm-options-title">Options</h2>
						<FormGroup id="vm-options" label="Features" choices="checkbox">
							<div class="pf-v6-c-check">
								<input
									class="pf-v6-c-check__input"
									type="checkbox"
									id="vm-guest-agent"
									bind:checked={form.guestAgent}
									aria-describedby="vm-guest-agent-description"
								/>
								<label class="pf-v6-c-check__label" for="vm-guest-agent">Guest agent</label>
								<span class="pf-v6-c-check__description" id="vm-guest-agent-description">
									Lets qarax run commands in the guest (<code>qarax vm exec</code>). The guest has
									to run qarax-init, as VMs booted from an OCI image do.
								</span>
							</div>
							<div class="pf-v6-c-check">
								<input
									class="pf-v6-c-check__input"
									type="checkbox"
									id="vm-ha"
									bind:checked={form.ha}
									aria-describedby="vm-ha-description"
								/>
								<label class="pf-v6-c-check__label" for="vm-ha">High availability</label>
								<span class="pf-v6-c-check__description" id="vm-ha-description">
									If its host fails, qarax starts it again on another host. All of its disks have to
									be on shared storage.
								</span>
							</div>
						</FormGroup>
						<FormGroup id="vm-user-data" label="Cloud-init user data">
							<FormControl kind="textarea">
								<textarea
									id="vm-user-data"
									class="qx-mono"
									rows="6"
									spellcheck="false"
									placeholder="#cloud-config"
									bind:value={form.userData}
									aria-describedby="vm-user-data-helper"></textarea>
							</FormControl>
							{#snippet helper()}
								For guests that run cloud-init, such as cloud images. qarax hands it to the guest on
								a small read-only disk (NoCloud).
							{/snippet}
						</FormGroup>
					</section>

					{#if submit.kind === 'editing' && submit.error}
						<Notice tone="danger" title="Couldn't create the VM">
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
										<a class="pf-v6-c-button pf-m-link" href={resolve('/vms')}>
											<span class="pf-v6-c-button__text">Cancel</span>
										</a>
									</div>
								</div>
							</div>
						</div>
					</div>
				</form>
			{/snippet}
		</Loaded>
	</div>
</section>

<style>
	.notice {
		margin-bottom: var(--pf-t--global--spacer--lg);
	}

	/* A select is as wide as its longest option, which leaves no room for PatternFly's toggle icon. */
	.unit {
		min-width: 6rem;
	}

	/* Errors from a node can span lines (a command's output, say). */
	.message {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
</style>
