import type { Host } from './api/client';

/** What a host's status means for its VMs, and what to do when it isn't well. */
export function hostSituation(host: Host): string {
	switch (host.status) {
		case 'up':
			return 'It runs VMs and takes new ones.';
		case 'down':
			return `Not answering at ${host.address}:${host.port}. Check that qarax-node is running on it.`;
		case 'installation_failed':
			return host.installation_error ?? 'Its last deploy or upgrade failed.';
		case 'maintenance':
			return 'It keeps its VMs but takes no new ones until maintenance ends.';
		case 'installing':
			return 'A deploy or upgrade is running.';
		case 'unknown':
		case 'initializing':
			return "qarax hasn't heard from it yet.";
	}
}
