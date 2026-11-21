<script lang="ts">
	import { asset } from '$app/paths';
	import { session } from '#lib/session.svelte.ts';
	import Notice from './Notice.svelte';

	let { notice }: { notice: string | null } = $props();

	let token = $state('');
	let submitting = $state(false);
	let error: string | null = $state(null);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		submitting = true;
		error = await session.signIn(token.trim());
		submitting = false;
	}
</script>

<div class="pf-v6-c-login backdrop">
	<div class="pf-v6-c-login__container">
		<header class="pf-v6-c-login__header">
			<span class="brand">
				<img src={asset('favicon.svg')} alt="" width="40" height="40" />
				qarax
			</span>
		</header>
		<main class="pf-v6-c-login__main">
			<div class="pf-v6-c-login__main-header">
				<h1 class="pf-v6-c-title pf-m-3xl">Sign in</h1>
				<p class="pf-v6-c-login__main-header-desc">
					Paste one of the API tokens set in the control plane's <code>AUTH_TOKENS</code>.
				</p>
			</div>
			<div class="pf-v6-c-login__main-body">
				<form class="pf-v6-c-form" novalidate onsubmit={submit}>
					{#if error}
						<Notice tone="danger" title={error} />
					{:else if notice}
						<Notice tone="warning" title={notice} />
					{/if}
					<div class="pf-v6-c-form__group">
						<label class="pf-v6-c-form__label" for="token">
							<span class="pf-v6-c-form__label-text">API token</span>
						</label>
						<span class="pf-v6-c-form-control" class:pf-m-error={error !== null}>
							<input
								id="token"
								type="password"
								autocomplete="current-password"
								required
								aria-invalid={error !== null}
								bind:value={token}
							/>
						</span>
					</div>
					<div class="pf-v6-c-form__group pf-m-action">
						<div class="pf-v6-c-form__actions">
							<button
								class="pf-v6-c-button pf-m-primary pf-m-block"
								type="submit"
								disabled={submitting || !token.trim()}
							>
								<span class="pf-v6-c-button__text">{submitting ? 'Signing in…' : 'Sign in'}</span>
							</button>
						</div>
					</div>
				</form>
			</div>
		</main>
	</div>
</div>

<style>
	/* PatternFly's login sits on a background; its secondary surface stands in for the image. */
	.backdrop {
		min-height: 100vh;
		background: var(--pf-t--global--background--color--secondary--default);
	}

	.brand {
		display: inline-flex;
		align-items: center;
		gap: var(--pf-t--global--spacer--md);
		font-family: var(--pf-t--global--font--family--heading);
		font-size: var(--pf-t--global--font--size--heading--h1);
		font-weight: var(--pf-t--global--font--weight--heading--bold);
	}
</style>
