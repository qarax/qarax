<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { api, call, type NewNetwork } from '#lib/api/client.ts';
	import FormControl from '#lib/components/FormControl.svelte';
	import FormGroup from '#lib/components/FormGroup.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Notice from '#lib/components/Notice.svelte';

	/** The network types qarax-node sets up (`qarax network create --network-type`). */
	const TYPES = [
		{
			value: 'bridge',
			label: 'Bridge',
			about:
				'A bridge on each host it is attached to, with DHCP for its VMs and NAT out of the host.'
		},
		{
			value: 'isolated',
			label: 'Isolated',
			about:
				'Like a bridge, and traffic between hosts within its VPC is routed rather than sent through NAT.'
		},
		{
			value: 'passt',
			label: 'passt',
			about:
				"Each VM's traffic goes through passt, in user space: its hosts need no bridge, DHCP or NAT."
		}
	] as const;

	let form = $state({
		name: '',
		subnet: '',
		gateway: '',
		dns: '',
		type: 'bridge' as (typeof TYPES)[number]['value'],
		vpc: ''
	});

	const IPV4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/;
	const isIpv4 = (text: string) =>
		IPV4.test(text) && text.split('.').every((part) => Number(part) <= 255);
	const isCidr = (text: string) => {
		const [address, bits, ...rest] = text.split('/');
		return (
			rest.length === 0 && isIpv4(address) && /^\d{1,2}$/.test(bits ?? '') && Number(bits) <= 32
		);
	};

	type Field = 'name' | 'subnet' | 'gateway' | 'dns';
	type Checked =
		| { kind: 'valid'; request: NewNetwork }
		| { kind: 'invalid'; errors: Partial<Record<Field, string>> };

	const checked: Checked = $derived.by(() => {
		const errors: Partial<Record<Field, string>> = {};
		const name = form.name.trim();
		const subnet = form.subnet.trim();
		const gateway = form.gateway.trim();
		const dns = form.dns.trim();
		if (!name) errors.name = 'Give the network a name.';
		if (!isCidr(subnet)) errors.subnet = 'Enter the subnet in CIDR notation, such as 10.0.0.0/24.';
		if (gateway && !isIpv4(gateway)) errors.gateway = 'Enter an IPv4 address, such as 10.0.0.1.';
		if (dns && !isIpv4(dns)) errors.dns = 'Enter an IPv4 address, such as 10.0.0.1.';
		if (Object.keys(errors).length) return { kind: 'invalid', errors };
		return {
			kind: 'valid',
			request: {
				name,
				subnet,
				gateway: gateway || null,
				dns: dns || null,
				type: form.type,
				vpc_name: form.vpc.trim() || null
			}
		};
	});

	type Submit = { kind: 'editing'; tried: boolean; error: string | null } | { kind: 'creating' };
	let submit: Submit = $state({ kind: 'editing', tried: false, error: null });
	const errors = $derived(
		submit.kind === 'editing' && submit.tried && checked.kind === 'invalid' ? checked.errors : {}
	);
	const FIELDS: Field[] = ['name', 'subnet', 'gateway', 'dns'];

	const describedBy = (id: string, error: string | undefined, helper = false) =>
		error || helper ? `${id}-helper` : undefined;

	async function create(event: SubmitEvent) {
		event.preventDefault();
		if (checked.kind === 'invalid') {
			submit = { kind: 'editing', tried: true, error: null };
			await tick();
			const first = FIELDS.find((field) => field in checked.errors);
			if (first) document.getElementById(`network-${first}`)?.focus();
			return;
		}
		submit = { kind: 'creating' };
		try {
			const id = await call(api.POST('/v1/networks', { body: checked.request, parseAs: 'text' }));
			await goto(resolve('/networks/[id]', { id }));
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
					<a href={resolve('/networks')} class="pf-v6-c-breadcrumb__link">Networks</a>
				</li>
				<li class="pf-v6-c-breadcrumb__item">
					<span class="pf-v6-c-breadcrumb__item-divider"><Icon name="chevronRight" /></span>
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page"
						>Create network</span
					>
				</li>
			</ol>
		</nav>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Create a network</h1>
		<p class="pf-v6-c-content--p qx-subtle">
			qarax hands VMs on it addresses from its subnet. It carries traffic on the hosts you attach it
			to, which you do on its page next.
		</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<form class="pf-v6-c-form pf-m-limit-width" novalidate onsubmit={create}>
			<FormGroup id="network-name" label="Name" required error={errors.name}>
				<FormControl invalid={!!errors.name}>
					<input
						type="text"
						id="network-name"
						required
						autocomplete="off"
						spellcheck="false"
						bind:value={form.name}
						aria-invalid={errors.name ? 'true' : undefined}
						aria-describedby={describedBy('network-name', errors.name)}
					/>
				</FormControl>
			</FormGroup>
			<FormGroup id="network-subnet" label="Subnet" required error={errors.subnet}>
				<FormControl invalid={!!errors.subnet}>
					<input
						type="text"
						id="network-subnet"
						class="qx-mono"
						required
						autocomplete="off"
						spellcheck="false"
						placeholder="10.0.0.0/24"
						bind:value={form.subnet}
						aria-invalid={errors.subnet ? 'true' : undefined}
						aria-describedby={describedBy('network-subnet', errors.subnet)}
					/>
				</FormControl>
			</FormGroup>
			<FormGroup id="network-gateway" label="Gateway" error={errors.gateway}>
				<FormControl invalid={!!errors.gateway}>
					<input
						type="text"
						id="network-gateway"
						class="qx-mono"
						autocomplete="off"
						spellcheck="false"
						placeholder="10.0.0.1"
						bind:value={form.gateway}
						aria-invalid={errors.gateway ? 'true' : undefined}
						aria-describedby="network-gateway-helper"
					/>
				</FormControl>
				{#snippet helper()}
					The address its VMs send traffic for other networks to, usually the subnet's first.
				{/snippet}
			</FormGroup>
			<FormGroup id="network-dns" label="DNS server" error={errors.dns}>
				<FormControl invalid={!!errors.dns}>
					<input
						type="text"
						id="network-dns"
						class="qx-mono"
						autocomplete="off"
						spellcheck="false"
						bind:value={form.dns}
						aria-invalid={errors.dns ? 'true' : undefined}
						aria-describedby="network-dns-helper"
					/>
				</FormControl>
				{#snippet helper()}
					The server its VMs are told to resolve names with.
				{/snippet}
			</FormGroup>
			<FormGroup id="network-type" label="Type" choices="radio">
				{#each TYPES as type (type.value)}
					<div class="pf-v6-c-radio">
						<input
							class="pf-v6-c-radio__input"
							type="radio"
							id="network-type-{type.value}"
							value={type.value}
							bind:group={form.type}
							aria-describedby="network-type-{type.value}-description"
						/>
						<label class="pf-v6-c-radio__label" for="network-type-{type.value}">{type.label}</label>
						<span class="pf-v6-c-radio__description" id="network-type-{type.value}-description"
							>{type.about}</span
						>
					</div>
				{/each}
			</FormGroup>
			<FormGroup id="network-vpc" label="VPC">
				<FormControl>
					<input
						type="text"
						id="network-vpc"
						autocomplete="off"
						spellcheck="false"
						bind:value={form.vpc}
						aria-describedby="network-vpc-helper"
					/>
				</FormControl>
				{#snippet helper()}
					Networks with the same VPC name route between their subnets, across the hosts they are
					attached to. Leave it empty to keep this one to itself.
				{/snippet}
			</FormGroup>

			{#if submit.kind === 'editing' && submit.error}
				<Notice tone="danger" title="Couldn't create the network">
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
								<a class="pf-v6-c-button pf-m-link" href={resolve('/networks')}>
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
