<script lang="ts">
	import { resolve } from '$app/paths';
	import { allSandboxPools, allSandboxes, allVmTemplates, type Sandbox } from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { SANDBOX_STATUS } from '#lib/status.ts';
	import { ago } from '#lib/format.ts';
	import { expiry } from '#lib/sandboxes.ts';
	import CodeBlock from '#lib/components/CodeBlock.svelte';
	import Empty from '#lib/components/Empty.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import SearchInput from '#lib/components/SearchInput.svelte';
	import Status from '#lib/components/Status.svelte';

	const list = new Remote(async () => {
		const [sandboxes, pools, templates] = await Promise.all([
			allSandboxes(),
			allSandboxPools(),
			allVmTemplates()
		]);
		return {
			sandboxes,
			pools,
			templateNames: new Map(templates.map((template) => [template.id, template.name]))
		};
	});
	// A sandbox turns ready, and expires, without a VM status change to say so.
	keepFresh(list.refresh, () => true, 5_000);

	let filter = $state('');

	const matches = (sandbox: Sandbox, query: string) =>
		query
			.toLowerCase()
			.split(/\s+/)
			.filter(Boolean)
			.every((word) => sandbox.name.toLowerCase().includes(word));

	const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
</script>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<h1 class="pf-v6-c-content--h1 pf-m-page-title">Sandboxes</h1>
		<p class="pf-v6-c-content--p qx-subtle">
			Short-lived VMs made from a VM template, ready in seconds to run commands in. qarax deletes
			each one once it has gone unused for its idle timeout.
		</p>
	</div>
</section>

<section class="pf-v6-c-page__main-section">
	<div class="pf-v6-c-page__main-body">
		<Loaded remote={list} what="sandboxes">
			{#snippet children({ sandboxes, pools, templateNames })}
				{#if sandboxes.length === 0}
					<div>
						<Empty icon="sandbox" title="No sandboxes yet">
							A sandbox runs the code you hand it, an AI agent's say, in a VM of its own. Create one
							here, or from the CLI:
							<CodeBlock
								code={'qarax sandbox create --template <template> --name my-sandbox --wait\nqarax sandbox exec my-sandbox -- uname -a'}
							/>
							{#snippet footer()}
								<div class="pf-v6-c-empty-state__actions">
									<a class="pf-v6-c-button pf-m-primary" href={resolve('/sandboxes/new')}>
										<span class="pf-v6-c-button__text">Create sandbox</span>
									</a>
								</div>
							{/snippet}
						</Empty>
					</div>
				{:else}
					{@const shown = sandboxes.filter((sandbox) => matches(sandbox, filter))}
					<div>
						<div class="pf-v6-c-toolbar">
							<div class="pf-v6-c-toolbar__content">
								<div class="pf-v6-c-toolbar__content-section">
									<div class="pf-v6-c-toolbar__item search">
										<SearchInput bind:value={filter} label="Filter by name" />
									</div>
									<div class="pf-v6-c-toolbar__item">
										<a class="pf-v6-c-button pf-m-primary" href={resolve('/sandboxes/new')}>
											<span class="pf-v6-c-button__text">Create sandbox</span>
										</a>
									</div>
									<div class="pf-v6-c-toolbar__item pf-m-align-end qx-subtle count">
										{shown.length === sandboxes.length
											? plural(sandboxes.length, 'sandbox', 'sandboxes')
											: `${shown.length} of ${sandboxes.length} sandboxes`}
									</div>
								</div>
							</div>
						</div>
						{#if shown.length}
							<table class="pf-v6-c-table pf-m-grid-md" aria-label="Sandboxes">
								<thead class="pf-v6-c-table__thead">
									<tr class="pf-v6-c-table__tr">
										<th class="pf-v6-c-table__th" scope="col">Name</th>
										<th class="pf-v6-c-table__th" scope="col">Status</th>
										<th class="pf-v6-c-table__th" scope="col">Template</th>
										<th class="pf-v6-c-table__th" scope="col">IP address</th>
										<th class="pf-v6-c-table__th" scope="col">Deleted if unused</th>
										<th class="pf-v6-c-table__th" scope="col">Created</th>
									</tr>
								</thead>
								<tbody class="pf-v6-c-table__tbody">
									{#each shown as sandbox (sandbox.id)}
										{@const expires = expiry(sandbox)}
										<tr class="pf-v6-c-table__tr">
											<td class="pf-v6-c-table__td" data-label="Name">
												<a href={resolve('/sandboxes/[id]', { id: sandbox.id })}>{sandbox.name}</a>
											</td>
											<td class="pf-v6-c-table__td" data-label="Status"
												><Status look={SANDBOX_STATUS[sandbox.status]} /></td
											>
											<td class="pf-v6-c-table__td" data-label="Template">
												{#if sandbox.vm_template_id && templateNames.has(sandbox.vm_template_id)}
													{templateNames.get(sandbox.vm_template_id)}
												{:else}
													<span class="qx-subtle">None</span>
												{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="IP address">
												{#if sandbox.ip_address}<span class="qx-mono">{sandbox.ip_address}</span
													>{:else}<span class="qx-subtle">None</span>{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="Deleted if unused">
												{#if expires}{expires}{:else}<span class="qx-subtle"
														>Not while it isn't ready</span
													>{/if}
											</td>
											<td class="pf-v6-c-table__td" data-label="Created"
												>{ago(sandbox.created_at)}</td
											>
										</tr>
									{/each}
								</tbody>
							</table>
						{:else}
							<Empty icon="search" title="No matching sandboxes">
								No sandbox's name matches “{filter}”.
								{#snippet footer()}
									<div class="pf-v6-c-empty-state__actions">
										<button
											class="pf-v6-c-button pf-m-link"
											type="button"
											onclick={() => (filter = '')}
										>
											<span class="pf-v6-c-button__text">Clear the filter</span>
										</button>
									</div>
								{/snippet}
							</Empty>
						{/if}
					</div>
				{/if}

				<div class="pools">
					<h2 class="pf-v6-c-content--h2">Pools</h2>
					<p class="pf-v6-c-content--p qx-subtle">
						A pool keeps sandboxes of a template booted ahead of time, so a new one is ready the
						moment it is asked for.
					</p>
					{#if pools.length === 0}
						<p class="pf-v6-c-content--p">
							None yet. Give a template a pool from the CLI:
							<code>qarax sandbox pool set --template &lt;template&gt; --min-ready 2</code>, or with
							<code>--fork</code> to make its sandboxes by forking one booted VM of the template.
						</p>
					{:else}
						<table class="pf-v6-c-table pf-m-grid-md" aria-label="Sandbox pools">
							<thead class="pf-v6-c-table__thead">
								<tr class="pf-v6-c-table__tr">
									<th class="pf-v6-c-table__th" scope="col">Template</th>
									<th class="pf-v6-c-table__th" scope="col">Ready</th>
									<th class="pf-v6-c-table__th" scope="col">Provisioning</th>
									<th class="pf-v6-c-table__th" scope="col">Failed</th>
									<th class="pf-v6-c-table__th" scope="col">Makes them by</th>
								</tr>
							</thead>
							<tbody class="pf-v6-c-table__tbody">
								{#each pools as pool (pool.id)}
									{@const readySeeds = pool.seeds.filter((seed) => seed.status === 'ready').length}
									<tr class="pf-v6-c-table__tr">
										<td class="pf-v6-c-table__td" data-label="Template">{pool.vm_template_name}</td>
										<td class="pf-v6-c-table__td" data-label="Ready">
											{pool.current_ready}
											<span class="qx-subtle">of {pool.min_ready} kept ready</span>
										</td>
										<td class="pf-v6-c-table__td" data-label="Provisioning"
											>{pool.current_provisioning}</td
										>
										<td class="pf-v6-c-table__td" data-label="Failed">{pool.current_error}</td>
										<td class="pf-v6-c-table__td" data-label="Makes them by">
											{#if pool.fork}
												Forking {pool.seed_hosts === 1
													? 'a seed'
													: `seeds on ${pool.seed_hosts} hosts`}
												<span class="qx-subtle">({readySeeds} of {pool.seed_hosts} ready)</span>
											{:else}
												Booting each one
											{/if}
										</td>
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
	.count {
		align-self: center;
	}

	.search {
		min-width: min(22rem, 100%);
	}

	.pools {
		margin-top: var(--pf-t--global--spacer--2xl);
	}

	.pools p {
		margin-block: var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md);
	}
</style>
