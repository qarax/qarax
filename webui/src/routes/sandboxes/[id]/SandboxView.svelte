<script lang="ts">
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import {
		ApiError,
		allHosts,
		allVmTemplates,
		api,
		call,
		type ExecResult
	} from '#lib/api/client.ts';
	import { Remote, keepFresh } from '#lib/remote.svelte.ts';
	import { SANDBOX_STATUS, SANDBOX_STATUS_HELP, vmStatusLook } from '#lib/status.ts';
	import { ago, seconds } from '#lib/format.ts';
	import { expiry } from '#lib/sandboxes.ts';
	import ConfirmModal from '#lib/components/ConfirmModal.svelte';
	import Empty from '#lib/components/Empty.svelte';
	import Fact from '#lib/components/Fact.svelte';
	import FormControl from '#lib/components/FormControl.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import Status from '#lib/components/Status.svelte';
	import StatusLabel from '#lib/components/StatusLabel.svelte';

	let { id }: { id: string } = $props();

	// The page re-creates this view for another sandbox ({#key}), so the id is fixed.
	const sandboxId = untrack(() => id);
	const path = { params: { path: { sandbox_id: sandboxId } } };

	/** A sandbox can go while its page is open: qarax deletes it once it goes unused. */
	const detail = new Remote(async () => {
		try {
			const sandbox = await call(api.GET('/v1/sandboxes/{sandbox_id}', path));
			const [vm, hosts, templates] = await Promise.all([
				call(api.GET('/v1/vms/{vm_id}', { params: { path: { vm_id: sandbox.vm_id } } })),
				allHosts(),
				allVmTemplates()
			]);
			return {
				kind: 'found' as const,
				sandbox,
				vm,
				host: hosts.find((host) => host.id === vm.host_id),
				template: templates.find((template) => template.id === sandbox.vm_template_id)
			};
		} catch (e) {
			// The sandbox, or its VM, which a delete removes first.
			if (e instanceof ApiError && e.status === 404) return { kind: 'gone' as const };
			throw e;
		}
	});
	const vmId = $derived(
		'data' in detail.state && detail.state.data.kind === 'found'
			? detail.state.data.sandbox.vm_id
			: null
	);
	// A sandbox turns ready, and expires, without a VM status change to say so.
	keepFresh(detail.refresh, (event) => event.vm_id === vmId, 5_000);

	type ActionState =
		| { kind: 'idle' }
		| { kind: 'confirming-delete' }
		| { kind: 'running'; action: 'keepalive' | 'fork' | 'delete' }
		| { kind: 'failed'; message: string };
	let action: ActionState = $state({ kind: 'idle' });

	const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

	async function keepAlive() {
		action = { kind: 'running', action: 'keepalive' };
		try {
			await call(api.POST('/v1/sandboxes/{sandbox_id}/keepalive', path));
			action = { kind: 'idle' };
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
		}
		void detail.refresh();
	}

	/** One fork, ready when the request returns; its page is where to go next. */
	async function fork() {
		action = { kind: 'running', action: 'fork' };
		try {
			const [forked] = await call(
				api.POST('/v1/sandboxes/{sandbox_id}/fork', { ...path, body: { count: 1 } })
			);
			await goto(resolve('/sandboxes/[id]', { id: forked.id }));
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
		}
	}

	async function remove() {
		action = { kind: 'running', action: 'delete' };
		try {
			await call(api.DELETE('/v1/sandboxes/{sandbox_id}', path));
			await goto(resolve('/sandboxes'));
		} catch (e) {
			action = { kind: 'failed', message: message(e) };
		}
	}

	const PROGRESS = { keepalive: 'Keeping alive…', fork: 'Forking…', delete: 'Deleting…' };
	const LABEL = { keepalive: 'Keep alive', fork: 'Fork', delete: 'Delete' };
	const label = (name: keyof typeof LABEL) =>
		action.kind === 'running' && action.action === name ? PROGRESS[name] : LABEL[name];

	// Commands run through the guest's shell, so a line reads as it would in a
	// terminal; the API itself takes the command's arguments.
	const EXEC_TIMEOUT_SECS = 60;
	const RUNS_KEPT = 10;

	type Run = {
		seq: number;
		command: string;
		state:
			| { kind: 'running' }
			| { kind: 'done'; result: ExecResult }
			| { kind: 'failed'; error: string };
	};
	/** The latest commands run from this page, newest first. */
	let runs: Run[] = $state([]);
	let command = $state('');
	let seq = 0;
	const running = $derived(runs.some((run) => run.state.kind === 'running'));

	async function exec(event: SubmitEvent) {
		event.preventDefault();
		const line = command.trim();
		if (!line || running) return;
		const run = seq++;
		const started: Run = { seq: run, command: line, state: { kind: 'running' } };
		runs = [started, ...runs].slice(0, RUNS_KEPT);
		command = '';
		let state: Run['state'];
		try {
			const result = await call(
				api.POST('/v1/sandboxes/{sandbox_id}/exec', {
					...path,
					body: { command: ['/bin/sh', '-c', line], timeout_secs: EXEC_TIMEOUT_SECS }
				})
			);
			state = { kind: 'done', result };
		} catch (e) {
			state = { kind: 'failed', error: message(e) };
		}
		runs = runs.map((r) => (r.seq === run ? { ...r, state } : r));
		// A command restarts its idle clock.
		void detail.refresh();
	}

	/** Output ends in a newline, which a code block would show as an empty last line. */
	const withoutLastNewline = (text: string) => text.replace(/\n$/, '');

	function exitLook(result: ExecResult) {
		if (result.timed_out)
			return { tone: 'fault' as const, text: `Timed out after ${EXEC_TIMEOUT_SECS} s` };
		return {
			tone: result.exit_code === 0 ? ('run' as const) : ('fault' as const),
			text: `Exit code ${result.exit_code}`
		};
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
					<span class="pf-v6-c-breadcrumb__link pf-m-current" aria-current="page">
						{'data' in detail.state && detail.state.data.kind === 'found'
							? detail.state.data.sandbox.name
							: 'Sandbox'}
					</span>
				</li>
			</ol>
		</nav>
	</div>
</section>

<Loaded remote={detail} what="this sandbox">
	{#snippet children(data)}
		{#if data.kind === 'gone'}
			<section class="pf-v6-c-page__main-section">
				<div class="pf-v6-c-page__main-body">
					<Empty icon="sandbox" title="This sandbox is gone">
						qarax deletes a sandbox once it has gone unused for its idle timeout, and its VM with
						it. Someone may have deleted it, too.
						{#snippet footer()}
							<div class="pf-v6-c-empty-state__actions">
								<a class="pf-v6-c-button pf-m-primary" href={resolve('/sandboxes')}>
									<span class="pf-v6-c-button__text">See the sandboxes</span>
								</a>
							</div>
						{/snippet}
					</Empty>
				</div>
			</section>
		{:else}
			{@const { sandbox, vm, host, template } = data}
			{@const ready = sandbox.status === 'ready'}
			{@const expires = expiry(sandbox)}
			{@const busy = action.kind === 'running'}
			<section class="pf-v6-c-page__main-section">
				<div class="pf-v6-c-page__main-body">
					<div class="pf-v6-l-split pf-m-gutter pf-m-wrap">
						<div class="pf-v6-l-split__item pf-m-fill">
							<div class="title">
								<h1 class="pf-v6-c-title pf-m-2xl">{sandbox.name}</h1>
								<StatusLabel look={SANDBOX_STATUS[sandbox.status]} />
							</div>
							<p class="pf-v6-c-content--p qx-subtle">{SANDBOX_STATUS_HELP[sandbox.status]}</p>
						</div>
						<div class="pf-v6-l-split__item actions">
							{#if ready}
								<button
									class="pf-v6-c-button pf-m-secondary"
									type="button"
									disabled={busy}
									onclick={fork}
								>
									<span class="pf-v6-c-button__text">{label('fork')}</span>
								</button>
								<button
									class="pf-v6-c-button pf-m-secondary"
									type="button"
									disabled={busy}
									onclick={keepAlive}
								>
									<span class="pf-v6-c-button__text">{label('keepalive')}</span>
								</button>
							{/if}
							<button
								class="pf-v6-c-button pf-m-link pf-m-danger"
								type="button"
								disabled={busy}
								onclick={() => (action = { kind: 'confirming-delete' })}
							>
								<span class="pf-v6-c-button__text">{label('delete')}</span>
							</button>
						</div>
					</div>
					{#if sandbox.status === 'error' && sandbox.error_message}
						<div class="failed">
							<Notice tone="danger" title="It didn't get ready"
								><span class="message">{sandbox.error_message}</span></Notice
							>
						</div>
					{/if}
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
						<div class="pf-v6-l-grid__item pf-m-12-col pf-m-5-col-on-xl">
							<div class="pf-v6-c-card pf-m-full-height">
								<div class="pf-v6-c-card__title">
									<h2 class="pf-v6-c-card__title-text">Sandbox</h2>
								</div>
								<div class="pf-v6-c-card__body">
									<dl class="pf-v6-c-description-list pf-m-horizontal">
										<Fact term="VM">
											<a href={resolve('/vms/[id]', { id: vm.id })}>{vm.name}</a>
											<span class="vm-status"><Status look={vmStatusLook(vm.status)} /></span>
										</Fact>
										<Fact term="Host">
											{#if host}
												<a href={resolve('/hosts/[id]', { id: host.id })}>{host.name}</a>
											{:else}
												<span class="qx-subtle">Not placed yet</span>
											{/if}
										</Fact>
										<Fact term="Template">
											{#if template}{template.name}{:else}<span class="qx-subtle">None</span>{/if}
										</Fact>
										<Fact term="IP address">
											{#if sandbox.ip_address}<span class="qx-mono">{sandbox.ip_address}</span
												>{:else}<span class="qx-subtle">None</span>{/if}
										</Fact>
										<Fact term="Idle timeout">
											{seconds(sandbox.idle_timeout_secs)}{#if expires}<span class="qx-subtle"
													>, deleted {expires} unless it's used</span
												>{/if}
										</Fact>
										<Fact term="Last used">{ago(sandbox.last_activity_at)}</Fact>
										<Fact term="Created">{ago(sandbox.created_at)}</Fact>
										<Fact term="ID"><span class="qx-mono">{sandbox.id}</span></Fact>
									</dl>
								</div>
							</div>
						</div>
						<div class="pf-v6-l-grid__item pf-m-12-col pf-m-7-col-on-xl">
							<div class="pf-v6-c-card pf-m-full-height">
								<div class="pf-v6-c-card__title">
									<h2 class="pf-v6-c-card__title-text">Run a command</h2>
								</div>
								<div class="pf-v6-c-card__body">
									{#if ready}
										<form class="pf-v6-c-form" novalidate onsubmit={exec}>
											<div class="pf-v6-c-form__group">
												<div class="pf-v6-c-form__group-control">
													<div class="pf-v6-c-input-group">
														<div class="pf-v6-c-input-group__item pf-m-fill">
															<FormControl>
																<input
																	type="text"
																	class="qx-mono"
																	autocomplete="off"
																	spellcheck="false"
																	placeholder="uname -a"
																	aria-label="Command"
																	aria-describedby="exec-helper"
																	bind:value={command}
																/>
															</FormControl>
														</div>
														<div class="pf-v6-c-input-group__item">
															<button
																class="pf-v6-c-button pf-m-primary"
																type="submit"
																disabled={running}
															>
																<span class="pf-v6-c-button__text"
																	>{running ? 'Running…' : 'Run'}</span
																>
															</button>
														</div>
													</div>
													<div class="pf-v6-c-form__helper-text">
														<div class="pf-v6-c-helper-text" id="exec-helper">
															<div class="pf-v6-c-helper-text__item">
																<span class="pf-v6-c-helper-text__item-text">
																	Commands run in the sandbox's shell (<code>/bin/sh -c</code>), for
																	up to {EXEC_TIMEOUT_SECS}
																	s. For a longer one, use
																	<code
																		>qarax sandbox exec {sandbox.name} --timeout SECONDS -- …</code
																	>.
																</span>
															</div>
														</div>
													</div>
												</div>
											</div>
										</form>
									{:else}
										<p class="qx-subtle">
											{sandbox.status === 'provisioning'
												? 'Commands run once it is ready.'
												: "It can't run commands."}
										</p>
									{/if}
									{#each runs as run (run.seq)}
										<div class="run">
											<div class="run-head">
												<code class="qx-mono command">$ {run.command}</code>
												{#if run.state.kind === 'running'}
													<span class="qx-status qx-subtle">
														<svg
															class="pf-v6-c-spinner pf-m-sm"
															role="progressbar"
															viewBox="0 0 100 100"
															aria-label="Running"
														>
															<circle
																class="pf-v6-c-spinner__path"
																cx="50"
																cy="50"
																r="45"
																fill="none"
															/>
														</svg>
														Running
													</span>
												{:else if run.state.kind === 'done'}
													<StatusLabel look={exitLook(run.state.result)} />
												{/if}
											</div>
											{#if run.state.kind === 'failed'}
												<Notice tone="danger" title="It didn't run" plain
													><span class="message">{run.state.error}</span></Notice
												>
											{:else if run.state.kind === 'done'}
												{@const stdout = withoutLastNewline(run.state.result.stdout)}
												{@const stderr = withoutLastNewline(run.state.result.stderr)}
												{#if !stdout && !stderr}
													<p class="qx-subtle">No output.</p>
												{/if}
												{#if stdout}
													<div class="pf-v6-c-code-block">
														<div class="pf-v6-c-code-block__content">
															<pre class="pf-v6-c-code-block__pre output"><code
																	class="pf-v6-c-code-block__code">{stdout}</code
																></pre>
														</div>
													</div>
												{/if}
												{#if stderr}
													<p class="stream qx-subtle">stderr</p>
													<div class="pf-v6-c-code-block">
														<div class="pf-v6-c-code-block__content">
															<pre class="pf-v6-c-code-block__pre output"><code
																	class="pf-v6-c-code-block__code">{stderr}</code
																></pre>
														</div>
													</div>
												{/if}
											{/if}
										</div>
									{/each}
								</div>
							</div>
						</div>
					</div>
				</div>
			</section>

			{#if action.kind === 'confirming-delete'}
				<ConfirmModal
					title="Delete {sandbox.name}?"
					confirm="Delete"
					onconfirm={remove}
					oncancel={() => (action = { kind: 'idle' })}
				>
					Its VM is deleted with it, and so is what it holds: its memory, its processes and its
					files. This can't be undone.
				</ConfirmModal>
			{/if}
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

	.vm-status {
		margin-inline-start: var(--pf-t--global--spacer--sm);
	}

	.run {
		margin-top: var(--pf-t--global--spacer--lg);
	}

	.run-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: var(--pf-t--global--spacer--sm);
		margin-bottom: var(--pf-t--global--spacer--sm);
	}

	.command {
		overflow-wrap: anywhere;
	}

	.stream {
		margin-block: var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--xs);
		font-size: var(--pf-t--global--font--size--body--sm);
	}

	.output {
		max-height: 24rem;
		overflow: auto;
		white-space: pre-wrap;
	}
</style>
