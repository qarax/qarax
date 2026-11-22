<script lang="ts">
	import '#lib/patternfly.ts';
	import '../app.css';
	import { onMount, untrack } from 'svelte';
	import { afterNavigate } from '$app/navigation';
	import { page } from '$app/state';
	import { asset, resolve } from '$app/paths';
	import { session } from '#lib/session.svelte.ts';
	import { live } from '#lib/live.svelte.ts';
	import Empty from '#lib/components/Empty.svelte';
	import SignIn from '#lib/components/SignIn.svelte';
	import StatusIcon from '#lib/components/StatusIcon.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	// PatternFly shows the sidebar beside the page from its xl breakpoint;
	// below that it opens over the page from the masthead's toggle.
	const wide = matchMedia('(min-width: 75rem)');
	let sidebarOpen = $state(wide.matches);

	onMount(() => {
		void session.resume();
		const follow = () => (sidebarOpen = wide.matches);
		wide.addEventListener('change', follow);
		return () => wide.removeEventListener('change', follow);
	});

	afterNavigate(() => {
		if (!wide.matches) sidebarOpen = false;
	});

	const signedIn = $derived(session.state.kind === 'signed-in');
	$effect(() => {
		if (!signedIn) return;
		untrack(() => live.start());
		return () => live.stop();
	});

	const home = resolve('/');
	const sections = [
		{ href: home, label: 'Overview' },
		{ href: resolve('/vms'), label: 'Virtual machines' },
		{ href: resolve('/sandboxes'), label: 'Sandboxes' },
		{ href: resolve('/templates'), label: 'VM templates' },
		{ href: resolve('/hosts'), label: 'Hosts' },
		{ href: resolve('/networks'), label: 'Networks' },
		{ href: resolve('/storage'), label: 'Storage pools' }
	];

	const trim = (path: string) => path.replace(/\/$/, '');
	function isCurrent(href: string): boolean {
		const here = trim(page.url.pathname);
		const target = trim(href);
		return target === trim(home)
			? here === target
			: here === target || here.startsWith(`${target}/`);
	}
</script>

{#if session.state.kind === 'checking'}
	<div class="pf-v6-l-bullseye away">
		<svg
			class="pf-v6-c-spinner pf-m-xl"
			role="progressbar"
			viewBox="0 0 100 100"
			aria-label="Connecting to qarax"
		>
			<circle class="pf-v6-c-spinner__path" cx="50" cy="50" r="45" fill="none" />
		</svg>
	</div>
{:else if session.state.kind === 'unreachable'}
	<div class="pf-v6-l-bullseye away">
		<Empty icon="disconnected" title="Can't reach qarax">
			{session.state.message}
			{#snippet footer()}
				<div class="pf-v6-c-empty-state__actions">
					<button
						class="pf-v6-c-button pf-m-primary"
						type="button"
						onclick={() => session.resume()}
					>
						<span class="pf-v6-c-button__text">Try again</span>
					</button>
				</div>
			{/snippet}
		</Empty>
	</div>
{:else if session.state.kind === 'signed-out'}
	<SignIn notice={session.state.notice} />
{:else}
	<div class="pf-v6-c-page">
		<div class="pf-v6-c-skip-to-content">
			<a class="pf-v6-c-button pf-m-primary" href="#main-content">
				<span class="pf-v6-c-button__text">Skip to content</span>
			</a>
		</div>
		<header class="pf-v6-c-masthead">
			<div class="pf-v6-c-masthead__main">
				<span class="pf-v6-c-masthead__toggle">
					<button
						class="pf-v6-c-button pf-m-hamburger pf-m-plain"
						class:pf-m-expanded={sidebarOpen}
						type="button"
						aria-label="Global navigation"
						aria-expanded={sidebarOpen}
						aria-controls="primary-sidebar"
						onclick={() => (sidebarOpen = !sidebarOpen)}
					>
						<span class="pf-v6-c-button__icon">
							<svg
								viewBox="0 0 10 10"
								class="pf-v6-c-button--hamburger-icon pf-v6-svg"
								width="1em"
								height="1em"
							>
								<path class="pf-v6-c-button--hamburger-icon--top" d="M1,1 L9,1" />
								<path class="pf-v6-c-button--hamburger-icon--middle" d="M1,5 L9,5" />
								<path class="pf-v6-c-button--hamburger-icon--arrow" d="M1,5 L1,5 L1,5" />
								<path class="pf-v6-c-button--hamburger-icon--bottom" d="M9,9 L1,9" />
							</svg>
						</span>
					</button>
				</span>
				<div class="pf-v6-c-masthead__brand">
					<a class="pf-v6-c-masthead__logo brand" href={home}>
						<img src={asset('favicon.svg')} alt="" width="28" height="28" />
						<span>qarax</span>
					</a>
				</div>
			</div>
			<div class="pf-v6-c-masthead__content">
				<div class="pf-v6-c-toolbar pf-m-static">
					<div class="pf-v6-c-toolbar__content">
						<div class="pf-v6-c-toolbar__content-section">
							<div class="pf-v6-c-toolbar__item pf-m-align-end">
								<span class="qx-status live">
									{#if live.state.kind === 'open'}
										<StatusIcon tone="run" /> Live updates
									{:else if live.state.kind === 'retrying'}
										<StatusIcon tone="hold" /> Reconnecting…
									{:else}
										<StatusIcon tone="busy" /> Connecting…
									{/if}
								</span>
							</div>
							{#if session.token}
								<div class="pf-v6-c-toolbar__item">
									<button
										class="pf-v6-c-button pf-m-plain"
										type="button"
										onclick={() => session.signOut()}
									>
										<span class="pf-v6-c-button__text">Sign out</span>
									</button>
								</div>
							{/if}
						</div>
					</div>
				</div>
			</div>
		</header>
		<div
			class="pf-v6-c-page__sidebar"
			class:pf-m-expanded={sidebarOpen}
			class:pf-m-collapsed={!sidebarOpen}
			id="primary-sidebar"
		>
			<div class="pf-v6-c-page__sidebar-main">
				<div class="pf-v6-c-page__sidebar-body">
					<nav class="pf-v6-c-nav" aria-label="Global">
						<ul class="pf-v6-c-nav__list" role="list">
							{#each sections as section (section.href)}
								{@const current = isCurrent(section.href)}
								<li class="pf-v6-c-nav__item">
									<a
										href={section.href}
										class="pf-v6-c-nav__link"
										class:pf-m-current={current}
										aria-current={current ? 'page' : undefined}
									>
										<span class="pf-v6-c-nav__link-text">{section.label}</span>
									</a>
								</li>
							{/each}
						</ul>
					</nav>
				</div>
			</div>
		</div>
		<div class="pf-v6-c-page__main-container" tabindex="-1">
			<main class="pf-v6-c-page__main" tabindex="-1" id="main-content">
				{@render children()}
			</main>
		</div>
	</div>
{/if}

<style>
	.away {
		min-height: 100vh;
	}

	.brand {
		display: inline-flex;
		align-items: center;
		gap: var(--pf-t--global--spacer--sm);
		font-family: var(--pf-t--global--font--family--heading);
		font-size: var(--pf-t--global--font--size--heading--h3);
		font-weight: var(--pf-t--global--font--weight--heading--bold);
		color: var(--pf-t--global--text--color--regular);
		text-decoration: none;
	}

	.live {
		color: var(--pf-t--global--text--color--subtle);
		font-size: var(--pf-t--global--font--size--body--sm);
	}
</style>
