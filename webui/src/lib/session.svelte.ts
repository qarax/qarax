const STORAGE_KEY = 'qarax.token';

/**
 * Whether the UI may use the API. `signed-in` carries the token the API
 * accepted, or null when the control plane doesn't ask for one.
 */
export type SessionState =
	| { kind: 'checking' }
	| { kind: 'unreachable'; message: string }
	| { kind: 'signed-out'; notice: string | null }
	| { kind: 'signed-in'; token: string | null };

type Probe = { kind: 'accepted' } | { kind: 'rejected' } | { kind: 'unreachable'; message: string };

/** What the control plane makes of a token (or of none). */
async function probe(token: string | null): Promise<Probe> {
	let response: Response;
	try {
		response = await fetch('/v1/hosts?limit=1', {
			headers: token ? { Authorization: `Bearer ${token}` } : {}
		});
	} catch {
		return {
			kind: 'unreachable',
			message:
				"The control plane didn't answer. Check that qarax-server is running and that this browser can reach it."
		};
	}
	if (response.ok) return { kind: 'accepted' };
	if (response.status === 401) return { kind: 'rejected' };
	const text = (await response.text()).trim();
	return {
		kind: 'unreachable',
		message: text || `The control plane answered ${response.status} ${response.statusText}.`
	};
}

class Session {
	state: SessionState = $state({ kind: 'checking' });

	get token(): string | null {
		return this.state.kind === 'signed-in' ? this.state.token : null;
	}

	/** Signs in with the token saved in this browser, if the control plane still takes it. */
	async resume() {
		this.state = { kind: 'checking' };
		const saved = localStorage.getItem(STORAGE_KEY);
		const result = await probe(saved);
		switch (result.kind) {
			case 'accepted':
				this.state = { kind: 'signed-in', token: saved };
				break;
			case 'rejected':
				localStorage.removeItem(STORAGE_KEY);
				this.state = {
					kind: 'signed-out',
					notice: saved ? 'The token saved in this browser is no longer accepted.' : null
				};
				break;
			case 'unreachable':
				this.state = { kind: 'unreachable', message: result.message };
				break;
		}
	}

	/** Resolves to why the token wasn't taken, or to null once signed in. */
	async signIn(token: string): Promise<string | null> {
		const result = await probe(token);
		switch (result.kind) {
			case 'accepted':
				localStorage.setItem(STORAGE_KEY, token);
				this.state = { kind: 'signed-in', token };
				return null;
			case 'rejected':
				return "The control plane didn't accept that token. Check it against AUTH_TOKENS and try again.";
			case 'unreachable':
				return result.message;
		}
	}

	signOut() {
		localStorage.removeItem(STORAGE_KEY);
		this.state = { kind: 'signed-out', notice: null };
	}

	/** The API refused the token in use. */
	expire() {
		if (this.state.kind !== 'signed-in') return;
		localStorage.removeItem(STORAGE_KEY);
		this.state = { kind: 'signed-out', notice: 'Your token is no longer accepted. Sign in again.' };
	}
}

export const session = new Session();
