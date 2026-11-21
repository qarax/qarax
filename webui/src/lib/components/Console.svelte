<script lang="ts">
	import { onMount } from 'svelte';
	import { Terminal } from '@xterm/xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import '@xterm/xterm/css/xterm.css';
	import StatusIcon from './StatusIcon.svelte';

	/** The VM's serial console, over /v1/vms/{id}/console/attach. */
	let { vmId, token }: { vmId: string; token: string | null } = $props();

	type Link = { kind: 'connecting' } | { kind: 'open' } | { kind: 'closed'; reason: string };

	let link: Link = $state({ kind: 'connecting' });
	let screen: HTMLDivElement;
	let terminal: Terminal | undefined;
	let socket: WebSocket | undefined;
	const encoder = new TextEncoder();

	const hex = (text: string) =>
		Array.from(encoder.encode(text), (byte) => byte.toString(16).padStart(2, '0')).join('');

	function connect() {
		socket?.close();
		terminal?.reset();
		link = { kind: 'connecting' };
		const url = new URL(`/v1/vms/${vmId}/console/attach`, location.href);
		url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
		// A browser can't set the Authorization header on a WebSocket, so the token
		// goes as a subprotocol (qarax/src/auth.rs); the server selects qarax.console.
		const protocols = ['qarax.console', ...(token ? [`qarax.bearer.${hex(token)}`] : [])];
		const ws = new WebSocket(url, protocols);
		ws.binaryType = 'arraybuffer';
		let opened = false;
		ws.onopen = () => {
			opened = true;
			link = { kind: 'open' };
			terminal?.focus();
		};
		ws.onmessage = (message) => {
			// Console output is binary; the server's own notices (an attach error) are text.
			terminal?.write(
				typeof message.data === 'string' ? message.data : new Uint8Array(message.data)
			);
		};
		ws.onclose = () => {
			if (socket !== ws) return;
			link = {
				kind: 'closed',
				reason: opened
					? 'The console connection closed.'
					: "Couldn't attach to the console. Check that the VM is still running, then try again."
			};
		};
		socket = ws;
	}

	onMount(() => {
		const fit = new FitAddon();
		const term = new Terminal({
			fontFamily: "'Red Hat Mono', ui-monospace, monospace",
			fontSize: 14,
			cursorBlink: true,
			scrollback: 5000,
			// A terminal stays dark in both themes, like PatternFly's code editor.
			theme: {
				background: '#151515',
				foreground: '#e0e0e0',
				cursor: '#92c5f9',
				selectionBackground: '#004d99'
			}
		});
		term.loadAddon(fit);
		term.onData((data) => {
			if (socket?.readyState === WebSocket.OPEN) socket.send(encoder.encode(data));
		});
		terminal = term;
		const resize = new ResizeObserver(() => fit.fit());
		// xterm measures its cell size when it opens, so the font must be in by then.
		void document.fonts.load("14px 'Red Hat Mono'").then(() => {
			if (terminal !== term) return;
			term.open(screen);
			fit.fit();
			resize.observe(screen);
			connect();
		});
		return () => {
			resize.disconnect();
			const ws = socket;
			socket = undefined;
			ws?.close();
			terminal = undefined;
			term.dispose();
		};
	});
</script>

<div class="console">
	<div class="inset">
		<div class="screen" bind:this={screen}></div>
	</div>
	<div class="bar">
		{#if link.kind === 'connecting'}
			<span class="qx-status"><StatusIcon tone="busy" />Connecting to the console…</span>
		{:else if link.kind === 'open'}
			<span class="qx-status"
				><StatusIcon tone="run" />Connected. What you type goes to the VM's serial console.</span
			>
		{:else}
			<span class="qx-status"><StatusIcon tone="idle" />{link.reason}</span>
			<button class="pf-v6-c-button pf-m-secondary" type="button" onclick={connect}>
				<span class="pf-v6-c-button__text">Reconnect</span>
			</button>
		{/if}
	</div>
</div>

<style>
	.console {
		border: 1px solid var(--pf-t--global--border--color--default);
		border-radius: var(--pf-t--global--border--radius--small);
		overflow: hidden;
		background: #151515;
	}

	/* FitAddon sizes the terminal to .screen's box, so the padding goes around it. */
	.inset {
		padding: var(--pf-t--global--spacer--sm);
	}

	.screen {
		height: min(31rem, calc(100vh - 24rem));
		min-height: 15rem;
	}

	.bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--pf-t--global--spacer--md);
		padding: var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md);
		background: var(--pf-t--global--background--color--secondary--default);
		border-top: 1px solid var(--pf-t--global--border--color--default);
		min-height: 3.25rem;
	}
</style>
