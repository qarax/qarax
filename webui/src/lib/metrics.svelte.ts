import { api, call } from './api/client';

/** How often a VM's counters are read, and how long the charts look back. */
export const SAMPLE_MS = 2000;
export const WINDOW_MS = 5 * 60_000;

/**
 * The counters each kind of device is charted by, in bytes, as Cloud
 * Hypervisor names them. A device is the kind whose counters it has.
 */
export const DEVICE_KINDS = {
	disk: {
		title: 'Disk',
		series: [
			{ key: 'read_bytes', label: 'Read' },
			{ key: 'write_bytes', label: 'Written' }
		]
	},
	network: {
		title: 'Network',
		series: [
			{ key: 'rx_bytes', label: 'Received' },
			{ key: 'tx_bytes', label: 'Sent' }
		]
	}
} as const;

export type DeviceKind = keyof typeof DEVICE_KINDS;

type Counters = Record<string, Record<string, number>>;

interface Sample {
	at: number;
	counters: Counters;
}

/** Bytes per second of each of a kind's series, over the interval ending `at`. */
export interface RatePoint {
	at: number;
	values: number[];
}

export interface DeviceRates {
	device: string;
	kind: DeviceKind;
	/** Oldest first; null where the line breaks (no reading, or the counters restarted). */
	points: (RatePoint | null)[];
	/** Each series' counter: what the device has moved since the VM booted. */
	totals: number[];
}

/** Reads a VM's counters every {@link SAMPLE_MS} while it runs, keeping {@link WINDOW_MS} of them. */
export class MetricsSampler {
	samples: Sample[] = $state.raw([]);
	error: string | null = $state(null);
	#vmId: string;

	constructor(vmId: string) {
		this.#vmId = vmId;
	}

	/** Samples until the returned function is called. */
	run(): () => void {
		let stopped = false;
		let timer: ReturnType<typeof setTimeout> | undefined;
		const tick = async () => {
			try {
				const metrics = await call(
					api.GET('/v1/vms/{vm_id}/metrics', { params: { path: { vm_id: this.#vmId } } })
				);
				if (stopped) return;
				const at = Date.now();
				this.samples = [
					...this.samples.filter((sample) => sample.at > at - WINDOW_MS - SAMPLE_MS),
					{ at, counters: metrics.counters }
				];
				this.error = null;
			} catch (e) {
				if (stopped) return;
				this.error = e instanceof Error ? e.message : String(e);
			}
			// The next read waits for this one, so a slow node never stacks requests.
			timer = setTimeout(tick, SAMPLE_MS);
		};
		void tick();
		return () => {
			stopped = true;
			clearTimeout(timer);
		};
	}
}

/** Two readings further apart than this have a gap between them, not a rate. */
const GAP_MS = 3 * SAMPLE_MS;

/** Each charted device in the latest reading, with its rates over the samples. */
export function deviceRates(samples: Sample[]): DeviceRates[] {
	const latest = samples.at(-1);
	if (!latest) return [];
	const devices: DeviceRates[] = [];
	for (const device of Object.keys(latest.counters).sort()) {
		const counters = latest.counters[device];
		const kind = (Object.keys(DEVICE_KINDS) as DeviceKind[]).find((name) =>
			DEVICE_KINDS[name].series.every((series) => series.key in counters)
		);
		if (!kind) continue;
		const keys = DEVICE_KINDS[kind].series.map((series) => series.key);
		const points: (RatePoint | null)[] = [];
		for (let i = 1; i < samples.length; i++) {
			const before = samples[i - 1];
			const after = samples[i];
			const from = before.counters[device];
			const to = after.counters[device];
			const seconds = (after.at - before.at) / 1000;
			const deltas = keys.map((key) => (to?.[key] ?? NaN) - (from?.[key] ?? NaN));
			// A counter that went down belongs to a VM that started again.
			const usable = after.at - before.at <= GAP_MS && deltas.every((delta) => delta >= 0);
			if (usable) points.push({ at: after.at, values: deltas.map((delta) => delta / seconds) });
			else if (points.length && points.at(-1) !== null) points.push(null);
		}
		devices.push({ device, kind, points, totals: keys.map((key) => counters[key]) });
	}
	return devices;
}
