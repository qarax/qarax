import type { Hypervisor } from './api/client';

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

/** How long ago an instant was: `12 s ago`, `5 min ago`. */
export function ago(iso: string, now = Date.now()): string {
	const seconds = Math.max(0, Math.round((now - Date.parse(iso)) / 1000));
	if (seconds < 5) return 'just now';
	if (seconds < 60) return `${seconds} s ago`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
	if (seconds < 86_400) return `${Math.floor(seconds / 3600)} h ago`;
	return `${Math.floor(seconds / 86_400)} d ago`;
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
