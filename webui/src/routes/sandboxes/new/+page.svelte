<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import {
		allNetworks,
		allVmTemplates,
		api,
		call,
		type NewSandbox,
		type VmTemplate
	} from '#lib/api/client.ts';
	import { Remote } from '#lib/remote.svelte.ts';
	import { templateSummary } from '#lib/sandboxes.ts';
	import CodeBlock from '#lib/components/CodeBlock.svelte';
	import Empty from '#lib/components/Empty.svelte';
	import FormControl from '#lib/components/FormControl.svelte';
	import FormGroup from '#lib/components/FormGroup.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';

	const describe = (template: VmTemplate) =>
		[`${templateSummary(template)}.`, template.description].filter(Boolean).join(' ');

	const choices = new Remote(async () => {
		const [templates, networks] = await Promise.all([allVmTemplates(), allNetworks()]);
		return { templates, networks };
	});
	onMount(choices.refresh);

	type Unit = 's' | 'min' | 'h';
	const UNIT_SECONDS: Record<Unit, number> = { s: 1, min: 60, h: 3600 };
	/** The most seconds the API takes (an i32). */
	const MAX_SECONDS = 2_147_483_647;

	let form = $state({
		// A template's page links here with ?template= to make a sandbox of it.
		templateId: page.url.searchParams.get('template') ?? '',
		name: '',
		/** Empty for the control plane's default. */
		idle: null as number | null,
		idleUnit: 'min' as Unit,
		/** Empty for the template's network, if it has one. */
		networkId: ''
	});

	type Field = 'templateId' | 'name' | 'idle';
	type Checked =
		| { kind: 'valid'; request: NewSandbox }
		| { kind: 'invalid'; errors: Partial<Record<Field, string>> };

	const checked: Checked = $derived.by(() => {
		const errors: Partial<Record<Field, string>> = {};
		if (!form.templateId) errors.templateId = 'Pick the template to make it from.';
		const name = form.name.trim();
		if (!name) errors.name = 'Give the sandbox a name.';
		const idle = form.idle === null ? null : form.idle * UNIT_SECONDS[form.idleUnit];
		if (idle !== null && !(Number.isInteger(idle) && idle >= 1 && idle <= MAX_SECONDS))
			errors.idle = 'Enter a whole number of seconds, 1 or more, or leave it empty.';
		if (Object.keys(errors).length) return { kind: 'invalid', errors };
		return {
			kind: 'valid',
			request: {
				vm_template_id: form.templateId,
				name,
				idle_timeout_secs: idle,
				network_id: form.networkId || null
			}
		};
	});

	type Submit = { kind: 'editing'; tried: boolean; error: string | null } | { kind: 'creating' };
	let submit: Submit = $state({ kind: 'editing', tried: false, error: null });
	const errors = $derived(
		submit.kind === 'editing' && submit.tried && checked.kind === 'invalid' ? checked.errors : {}
	);

	const FIELD_IDS: Record<Field, string> = {
		templateId: 'sandbox-template',
		name: 'sandbox-name',
		idle: 'sandbox-idle'
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
			const created = await call(api.POST('/v1/sandboxes', { body: checked.request }));
			await goto(resolve('/sandboxes/[id]', { id: created.id }));
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
					<a href={resolve('/sandboxes')} class="pf-v6-c-breadcrumb__link">Sandboxes</a>
				</li>
				<li class="pf-v6-c-breadcrumb__item">
					<span class="pf-v6-c-breadcrumb__item-divider"><Icon name="chevronRight" /></span>
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page"
						>Create sandbox</span
					>
				</li>
			</ol>
		</nav>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Create a sandbox</h1>
		<p class="pf-v6-c-content--p qx-subtle">
			qarax boots a VM from the template on a host with room for it, or hands out one its pool keeps
			ready. It runs commands once its guest agent answers.
		</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={choices} what="the VM templates">
			{#snippet children({ templates, networks })}
				{#if templates.length === 0}
					<Empty icon="template" title="No VM templates yet">
						A sandbox is made from a VM template: its hypervisor, its size and what it boots. Create
						one from the CLI, then come back here:
						<CodeBlock
							code={'qarax vm-template create --name agent --hypervisor firecracker --vcpus 1 --memory 268435456'}
						/>
					</Empty>
				{:else}
					{@const template = templates.find((t) => t.id === form.templateId)}
					{@const templateNetwork = networks.find((network) => network.id === template?.network_id)}
					{#snippet templateHelper()}
						{#if template}{describe(template)}{/if}
					{/snippet}
					<form class="pf-v6-c-form pf-m-limit-width" novalidate onsubmit={create}>
						<FormGroup
							id="sandbox-template"
							label="Template"
							required
							error={errors.templateId}
							helper={template ? templateHelper : undefined}
						>
							<FormControl kind="select" invalid={!!errors.templateId}>
								<select
									id="sandbox-template"
									required
									bind:value={form.templateId}
									aria-invalid={errors.templateId ? 'true' : undefined}
									aria-describedby={describedBy('sandbox-template', errors.templateId, !!template)}
								>
									<option value="" disabled>Pick a template</option>
									{#each templates as t (t.id)}
										<option value={t.id}>{t.name}</option>
									{/each}
								</select>
							</FormControl>
						</FormGroup>
						<FormGroup id="sandbox-name" label="Name" required error={errors.name}>
							<FormControl invalid={!!errors.name}>
								<input
									type="text"
									id="sandbox-name"
									required
									autocomplete="off"
									spellcheck="false"
									bind:value={form.name}
									aria-invalid={errors.name ? 'true' : undefined}
									aria-describedby={describedBy('sandbox-name', errors.name)}
								/>
							</FormControl>
						</FormGroup>
						<FormGroup id="sandbox-idle" label="Idle timeout" error={errors.idle}>
							<div class="pf-v6-c-input-group">
								<div class="pf-v6-c-input-group__item pf-m-fill">
									<FormControl invalid={!!errors.idle}>
										<input
											type="number"
											id="sandbox-idle"
											min="1"
											step="1"
											placeholder="The control plane's default"
											bind:value={form.idle}
											aria-invalid={errors.idle ? 'true' : undefined}
											aria-describedby="sandbox-idle-helper"
										/>
									</FormControl>
								</div>
								<div class="pf-v6-c-input-group__item unit">
									<FormControl kind="select">
										<select bind:value={form.idleUnit} aria-label="Idle timeout unit">
											<option value="s">seconds</option>
											<option value="min">minutes</option>
											<option value="h">hours</option>
										</select>
									</FormControl>
								</div>
							</div>
							{#snippet helper()}
								qarax deletes it once it has gone this long without a command or a keepalive. Left
								empty, the control plane's default applies (5 minutes unless it's configured
								otherwise).
							{/snippet}
						</FormGroup>
						<FormGroup id="sandbox-network" label="Network">
							<FormControl kind="select">
								<select
									id="sandbox-network"
									bind:value={form.networkId}
									aria-describedby="sandbox-network-helper"
								>
									<option value=""
										>{templateNetwork
											? `${templateNetwork.name} (the template's)`
											: 'No network'}</option
									>
									{#each networks.filter((network) => network.id !== templateNetwork?.id) as network (network.id)}
										<option value={network.id}>{network.name} ({network.subnet})</option>
									{/each}
								</select>
							</FormControl>
							{#snippet helper()}
								On a network it gets an address from its subnet, and can reach what the network
								does.
							{/snippet}
						</FormGroup>

						{#if submit.kind === 'editing' && submit.error}
							<Notice tone="danger" title="Couldn't create the sandbox">
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
											<a class="pf-v6-c-button pf-m-link" href={resolve('/sandboxes')}>
												<span class="pf-v6-c-button__text">Cancel</span>
											</a>
										</div>
									</div>
								</div>
							</div>
						</div>
					</form>
				{/if}
			{/snippet}
		</Loaded>
	</div>
</section>

<style>
	/* A select is as wide as its longest option, which leaves no room for PatternFly's toggle icon. */
	.unit {
		min-width: 8rem;
	}

	/* Errors from a node can span lines (a command's output, say). */
	.message {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
</style>
