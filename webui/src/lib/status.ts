import type {
	HostStatus,
	NetworkStatus,
	StoragePoolStatus,
	TransferStatus,
	VmStatus
} from './api/client.ts';

/**
 * How a status is drawn, with PatternFly's status icons (StatusIcon): `run`
 * ok, `hold` paused, `idle` off, `busy` in progress while a job holds it,
 * `fault` error, `unknown` unknown. Icon and word carry the status, colour
 * only adds to them.
 */
export type Tone = 'run' | 'hold' | 'idle' | 'busy' | 'fault' | 'unknown';

export interface StatusLook {
	tone: Tone;
	text: string;
}

/** In the order lists show them: running first, then what is changing, then stopped. */
export const VM_STATUS: Record<VmStatus, StatusLook> = {
	running: { tone: 'run', text: 'Running' },
	paused: { tone: 'hold', text: 'Paused' },
	pending: { tone: 'busy', text: 'Pending' },
	migrating: { tone: 'busy', text: 'Migrating' },
	committing: { tone: 'busy', text: 'Committing' },
	created: { tone: 'idle', text: 'Created' },
	shutdown: { tone: 'idle', text: 'Shut down' },
	unknown: { tone: 'unknown', text: 'Unknown' }
};

/** What a VM's status means, for someone new to it. */
export const VM_STATUS_HELP: Record<VmStatus, string> = {
	running: 'Its guest is up and running.',
	paused: 'Its vCPUs are stopped and its memory is kept. Resume it to carry on where it left off.',
	created: 'It has never been started. Start it to boot it on its host.',
	shutdown:
		'Its guest is off. Its disks and settings are kept, so it boots again when you start it.',
	pending:
		'qarax is preparing its image, starting it or restoring it. Its status changes when that is done.',
	migrating: 'It is moving to another host while it keeps running.',
	committing: 'Its disk is being saved as an image. Wait for that to finish before starting it.',
	unknown: "qarax can't tell its state: its host isn't reporting it."
};

/** The look of a status from the event stream, which carries it as a plain string. */
export function vmStatusLook(status: string): StatusLook {
	return status in VM_STATUS ? VM_STATUS[status as VmStatus] : { tone: 'unknown', text: status };
}

export const HOST_STATUS: Record<HostStatus, StatusLook> = {
	up: { tone: 'run', text: 'Up' },
	down: { tone: 'fault', text: 'Down' },
	installing: { tone: 'busy', text: 'Installing' },
	installation_failed: { tone: 'fault', text: 'Installation failed' },
	maintenance: { tone: 'hold', text: 'Maintenance' },
	initializing: { tone: 'busy', text: 'Initializing' },
	unknown: { tone: 'unknown', text: 'Unknown' }
};

export const POOL_STATUS: Record<StoragePoolStatus, StatusLook> = {
	active: { tone: 'run', text: 'Active' },
	inactive: { tone: 'idle', text: 'Inactive' },
	error: { tone: 'fault', text: 'Error' }
};

export const NETWORK_STATUS: Record<NetworkStatus, StatusLook> = {
	active: { tone: 'run', text: 'Active' },
	inactive: { tone: 'idle', text: 'Inactive' }
};

export const TRANSFER_STATUS: Record<TransferStatus, StatusLook> = {
	pending: { tone: 'busy', text: 'Pending' },
	running: { tone: 'busy', text: 'Copying' },
	completed: { tone: 'run', text: 'Done' },
	failed: { tone: 'fault', text: 'Failed' }
};
