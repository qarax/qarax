import type { Sandbox, VmTemplate } from './api/client';
import { HYPERVISOR, bytes, span } from './format';

/**
 * When qarax deletes a sandbox for going unused: its idle timeout after its
 * last activity. Only a ready sandbox expires (`list_expired` in
 * qarax/src/model/sandboxes.rs), and the reaper looks every few seconds.
 */
export function expiry(sandbox: Sandbox, now = Date.now()): string | null {
	if (sandbox.status !== 'ready') return null;
	const deadline = Date.parse(sandbox.last_activity_at) + sandbox.idle_timeout_secs * 1000;
	const left = Math.floor((deadline - now) / 1000);
	return left > 0 ? `in ${span(left)}` : 'any moment now';
}

/**
 * What a template makes, in a line: `Firecracker, 1 vCPU, 256 MiB`. A
 * sandbox runs on Firecracker unless its template says otherwise
 * (`resolve_sandbox_vm` in qarax/src/sandbox_runtime.rs); what the template
 * leaves out comes from the control plane's VM defaults.
 */
export function templateSummary(template: VmTemplate): string {
	const parts = [HYPERVISOR[template.hypervisor ?? 'firecracker']];
	if (template.boot_vcpus)
		parts.push(template.boot_vcpus === 1 ? '1 vCPU' : `${template.boot_vcpus} vCPUs`);
	if (template.memory_size) parts.push(bytes(template.memory_size));
	if (template.image_ref) parts.push(`boots ${template.image_ref}`);
	return parts.join(', ');
}
