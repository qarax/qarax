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

export async function runVmAction(vmId: string, action: VmAction): Promise<void> {
	const params = { params: { path: { vm_id: vmId } } };
	switch (action) {
		case 'start':
			await call(api.POST('/v1/vms/{vm_id}/start', params));
			return;
		case 'stop':
			await call(api.POST('/v1/vms/{vm_id}/stop', params));
			return;
		case 'pause':
			await call(api.POST('/v1/vms/{vm_id}/pause', params));
			return;
		case 'resume':
			await call(api.POST('/v1/vms/{vm_id}/resume', params));
			return;
		case 'force-stop':
			await call(api.POST('/v1/vms/{vm_id}/force-stop', params));
			return;
	}
}
