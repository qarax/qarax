import { api, call, type VmStatus } from './api/client';

export type VmAction = 'start' | 'stop' | 'pause' | 'resume' | 'force-stop';

/**
 * The actions offered for a VM in each status: those the control plane
 * allows from it (`VmOp::allowed_from` in qarax/src/model/vms.rs) that make
 * sense there. It has the last word; its refusal shows as an error.
 */
export const VM_ACTIONS: Record<VmStatus, VmAction[]> = {
	created: ['start'],
	shutdown: ['start'],
	running: ['stop', 'pause', 'force-stop'],
	paused: ['resume', 'stop', 'force-stop'],
	unknown: ['start', 'stop', 'force-stop'],
	pending: ['force-stop'],
	migrating: ['force-stop'],
	committing: ['force-stop']
};

export const ACTION_LABEL: Record<VmAction, string> = {
	start: 'Start',
	stop: 'Stop',
	pause: 'Pause',
	resume: 'Resume',
	'force-stop': 'Force stop'
};

export const ACTION_PROGRESS: Record<VmAction | 'delete', string> = {
	start: 'Starting…',
	stop: 'Stopping…',
	pause: 'Pausing…',
	resume: 'Resuming…',
	'force-stop': 'Force stopping…',
	delete: 'Deleting…'
};

/** An action the control plane took on: `finished` settles once it is done, rejecting with why it failed. */
export type Accepted = { finished: Promise<void> };

const DONE: Accepted = { finished: Promise.resolve() };

/**
 * Asks the control plane for an action, resolving once it has taken it on.
 * Every action but start is done by then. A start is a job that runs after
 * the answer (the VM is pending meanwhile), so its `finished` follows the job.
 */
export async function runVmAction(vmId: string, action: VmAction): Promise<Accepted> {
	const params = { params: { path: { vm_id: vmId } } };
	switch (action) {
		case 'start': {
			const { job_id } = await call(api.POST('/v1/vms/{vm_id}/start', params));
			return { finished: jobFinished(job_id) };
		}
		case 'stop':
			await call(api.POST('/v1/vms/{vm_id}/stop', params));
			return DONE;
		case 'pause':
			await call(api.POST('/v1/vms/{vm_id}/pause', params));
			return DONE;
		case 'resume':
			await call(api.POST('/v1/vms/{vm_id}/resume', params));
			return DONE;
		case 'force-stop':
			await call(api.POST('/v1/vms/{vm_id}/force-stop', params));
			return DONE;
	}
}

const JOB_POLL_MS = 1000;

/** Resolves when the job completes; rejects with the job's error when it fails. */
async function jobFinished(jobId: string): Promise<void> {
	const path = { params: { path: { job_id: jobId } } };
	for (;;) {
		const job = await call(api.GET('/v1/jobs/{job_id}', path));
		switch (job.status) {
			case 'completed':
				return;
			case 'failed':
				// The control plane always records why; the column is nullable for jobs still running.
				throw new Error(job.error ?? `Job ${jobId} failed.`);
			case 'pending':
			case 'running':
				await new Promise((resolve) => setTimeout(resolve, JOB_POLL_MS));
				break;
			default: {
				const status: never = job.status;
				throw new Error(`Job ${jobId} is ${String(status)}, which this page doesn't know.`);
			}
		}
	}
}
