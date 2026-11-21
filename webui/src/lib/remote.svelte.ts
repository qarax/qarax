import { onMount } from 'svelte';
import { live, type VmStatusEvent } from './live.svelte';

/** Data loaded from the API; `stale` keeps what loaded before when a refresh fails. */
export type RemoteState<T> =
	| { kind: 'loading' }
	| { kind: 'ready'; data: T }
	| { kind: 'failed'; error: string }
	| { kind: 'stale'; data: T; error: string };

export class Remote<T> {
	state: RemoteState<T> = $state({ kind: 'loading' });
	#load: () => Promise<T>;
	#inFlight = false;
	#again = false;

	constructor(load: () => Promise<T>) {
		this.#load = load;
	}

	/** Loads again; a call while a load runs makes it run once more after. */
	refresh = async () => {
		if (this.#inFlight) {
			this.#again = true;
			return;
		}
		this.#inFlight = true;
		try {
			do {
				this.#again = false;
				try {
					this.state = { kind: 'ready', data: await this.#load() };
				} catch (e) {
					const error = e instanceof Error ? e.message : String(e);
					this.state =
						'data' in this.state
							? { kind: 'stale', data: this.state.data, error }
							: { kind: 'failed', error };
				}
			} while (this.#again);
		} finally {
			this.#inFlight = false;
		}
	};
}

/**
 * Keeps a component's data current for its lifetime: refreshes it now, on
 * each VM status change `relevant` accepts, after the event stream
 * reconnects, and every `intervalMs` for what the stream doesn't carry (host
 * metrics, new VMs). Call it while the component initializes.
 */
export function keepFresh(
	refresh: () => void,
	relevant: (event: VmStatusEvent) => boolean = () => true,
	intervalMs = 15_000
) {
	onMount(() => {
		refresh();
		const unsubscribe = live.subscribe((event) => {
			if (event === null || relevant(event)) refresh();
		});
		const timer = setInterval(refresh, intervalMs);
		return () => {
			unsubscribe();
			clearInterval(timer);
		};
	});
}
