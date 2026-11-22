import type { Hypervisor, StorageObjectType, StoragePool, StoragePoolType } from './api/client';

const UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];

/** A byte count in binary units: `1 GiB`, `1.5 GiB`. */
export function bytes(n: number): string {
	let value = n;
	let unit = 0;
	while (value >= 1024 && unit < UNITS.length - 1) {
		value /= 1024;
		unit++;
	}
	return `${parseFloat(value.toFixed(1))} ${UNITS[unit]}`;
}

/** A while, in its largest unit, rounded down: `12 s`, `5 min`, `3 h`, `2 d`. */
export function span(seconds: number): string {
	if (seconds < 60) return `${seconds} s`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)} min`;
	if (seconds < 86_400) return `${Math.floor(seconds / 3600)} h`;
	return `${Math.floor(seconds / 86_400)} d`;
}

/** A configured length of time, exactly: `90 s`, `5 min`, `2 h`. */
export function seconds(n: number): string {
	if (n >= 3600 && n % 3600 === 0) return `${n / 3600} h`;
	if (n >= 60 && n % 60 === 0) return `${n / 60} min`;
	return `${n} s`;
}

/** How long ago an instant was: `12 s ago`, `5 min ago`. */
export function ago(iso: string, now = Date.now()): string {
	const elapsed = Math.max(0, Math.round((now - Date.parse(iso)) / 1000));
	return elapsed < 5 ? 'just now' : `${span(elapsed)} ago`;
}

/** The time of day of an instant, to the second. */
export function clock(iso: string): string {
	return new Date(iso).toLocaleTimeString([], {
		hour: '2-digit',
		minute: '2-digit',
		second: '2-digit'
	});
}

export const HYPERVISOR: Record<Hypervisor, string> = {
	cloud_hv: 'Cloud Hypervisor',
	firecracker: 'Firecracker'
};

export const POOL_TYPE: Record<StoragePoolType, string> = {
	local: 'Local',
	nfs: 'NFS',
	overlay_bd: 'OverlayBD',
	block: 'iSCSI block'
};

export const OBJECT_TYPE: Record<StorageObjectType, string> = {
	disk: 'Disk',
	kernel: 'Kernel',
	initrd: 'Initramfs',
	iso: 'ISO image',
	snapshot: 'Snapshot',
	database_backup: 'Database backup',
	oci_image: 'OCI image',
	overlaybd_upper: 'Writable image layer'
};

/**
 * Where a pool keeps its data, from its config (qarax/src/model/storage_pools.rs):
 * a directory, an NFS export or registry URL, or an iSCSI target. Null when the
 * config doesn't say, as a pool created through the API without one can.
 */
export function poolLocation(pool: StoragePool): string | null {
	const config = (
		typeof pool.config === 'object' && pool.config !== null ? pool.config : {}
	) as Record<string, unknown>;
	const text = (key: string) => {
		const value = config[key];
		return typeof value === 'string' ? value : null;
	};
	switch (pool.pool_type) {
		case 'local':
			return text('path');
		case 'nfs':
		case 'overlay_bd':
			return text('url');
		case 'block': {
			const iqn = text('iqn');
			const portal = text('portal');
			return iqn && portal ? `${iqn} at ${portal}` : null;
		}
	}
}
