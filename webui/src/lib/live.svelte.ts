import { session } from './session.svelte';

/** A VM's status change, as /v1/events streams it (qarax/src/model/events.rs). */
export interface VmStatusEvent {
	event: string;
	timestamp: string;
	vm_id: string;
	vm_name: string;
	previous_status: string;
	new_status: string;
	host_id: string | null;
	tags: string[];
}

/** A change as the feed keeps it: numbered in arrival order. */
export type LiveChange = VmStatusEvent & { seq: number };

export type LiveState =
	{ kind: 'off' } | { kind: 'connecting' } | { kind: 'open' } | { kind: 'retrying' };

/**
 * Called on each status change, and with null after reconnecting, when
 * changes made while the stream was down were missed.
 */
type Listener = (event: VmStatusEvent | null) => void;

const RECENT = 50;

/** The control plane's stream of VM status changes, while signed in. */
class Live {
	state: LiveState = $state({ kind: 'off' });
	/** The latest changes, newest first. */
	recent: LiveChange[] = $state([]);
	#seq = 0;
	#listeners = new Set<Listener>();
	#abort: AbortController | null = null;

	subscribe(listener: Listener): () => void {
		this.#listeners.add(listener);
		return () => this.#listeners.delete(listener);
	}

	start() {
		this.stop();
		const abort = new AbortController();
		this.#abort = abort;
		void this.#run(abort.signal);
	}

	stop() {
		this.#abort?.abort();
		this.#abort = null;
		this.state = { kind: 'off' };
	}

	async #run(signal: AbortSignal) {
		let delay = 1000;
		let connected = false;
		while (!signal.aborted) {
			this.state = { kind: 'connecting' };
			try {
				// EventSource can't send the Authorization header, so read the stream with fetch.
				const token = session.token;
				const response = await fetch('/v1/events', {
					headers: token ? { Authorization: `Bearer ${token}` } : {},
					signal
				});
				if (response.status === 401) {
					session.expire();
					return;
				}
				if (response.ok && response.body) {
					this.state = { kind: 'open' };
					delay = 1000;
					if (connected) this.#emit(null);
					connected = true;
					for await (const event of readEvents(response.body)) {
						this.recent = [{ ...event, seq: ++this.#seq }, ...this.recent].slice(0, RECENT);
						this.#emit(event);
					}
				}
			} catch {
				// A dropped connection, or stop(); the loop decides which.
			}
			if (signal.aborted) return;
			this.state = { kind: 'retrying' };
			await sleep(delay, signal);
			delay = Math.min(delay * 2, 30_000);
		}
	}

	#emit(event: VmStatusEvent | null) {
		for (const listener of this.#listeners) listener(event);
	}
}

/** The events of a server-sent event stream whose data is JSON. */
async function* readEvents(body: ReadableStream<Uint8Array>): AsyncGenerator<VmStatusEvent> {
	const reader = body.getReader();
	const decoder = new TextDecoder();
	let buffer = '';
	for (;;) {
		const { value, done } = await reader.read();
		if (done) return;
		buffer += decoder.decode(value, { stream: true });
		let end: number;
		while ((end = buffer.indexOf('\n\n')) >= 0) {
			const block = buffer.slice(0, end);
			buffer = buffer.slice(end + 2);
			const data = block
				.split('\n')
				.filter((line) => line.startsWith('data:'))
				.map((line) => line.slice(5).trimStart())
				.join('\n');
			if (data) yield JSON.parse(data) as VmStatusEvent;
		}
	}
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
	return new Promise((resolve) => {
		const timer = setTimeout(resolve, ms);
		signal.addEventListener('abort', () => {
			clearTimeout(timer);
			resolve();
		});
	});
}

export const live = new Live();
