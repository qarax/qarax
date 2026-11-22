import type { Hypervisor, NewVm, VmTemplate } from './api/client';
import { templateSize } from './templates';

/** What a new VM boots from. */
export type BootKind = 'image' | 'kernel' | 'disk';
/** `template`: the vCPUs and memory of the VM template it is made from. */
export type SizeKind = 'instance-type' | 'custom' | 'template';
export type MemoryUnit = 'MiB' | 'GiB';

/**
 * The create form as typed. Each choice keeps its own fields, so switching
 * between them doesn't lose what was entered; {@link toNewVm} reads only the
 * ones the choices select.
 */
export interface VmForm {
	/** The VM template it is made from; empty for none. */
	templateId: string;
	name: string;
	description: string;
	tags: string;
	boot: BootKind;
	imageRef: string;
	/** Empty for the control plane's default kernel. */
	bootSourceId: string;
	diskId: string;
	size: SizeKind;
	instanceTypeId: string;
	vcpus: number | null;
	memory: number | null;
	memoryUnit: MemoryUnit;
	hypervisor: Hypervisor;
	/** Empty for no network. */
	networkId: string;
	securityGroupIds: string[];
	guestAgent: boolean;
	ha: boolean;
	userData: string;
}

export const newVmForm = (): VmForm => ({
	templateId: '',
	name: '',
	description: '',
	tags: '',
	boot: 'image',
	imageRef: '',
	bootSourceId: '',
	diskId: '',
	size: 'custom',
	instanceTypeId: '',
	vcpus: 1,
	memory: 1,
	memoryUnit: 'GiB',
	hypervisor: 'cloud_hv',
	networkId: '',
	securityGroupIds: [],
	guestAgent: false,
	ha: false,
	userData: ''
});

export type Field =
	'name' | 'imageRef' | 'diskId' | 'instanceTypeId' | 'vcpus' | 'memory' | 'hypervisor';

export type Checked =
	{ kind: 'valid'; request: NewVm } | { kind: 'invalid'; errors: Partial<Record<Field, string>> };

const MIB = 1024 * 1024;
const UNIT_BYTES: Record<MemoryUnit, number> = { MiB: MIB, GiB: 1024 * MIB };

/**
 * Picks the template a VM is made from, and the size that goes with it: the
 * template's when it sets one. `template` is undefined for none.
 */
export function pickTemplate(form: VmForm, template: VmTemplate | undefined) {
	form.templateId = template?.id ?? '';
	form.size = template && templateSize(template) ? 'template' : 'custom';
}

/**
 * The request the form asks for, or what to fix in it first. A VM made from
 * `template` (the form's) boots what the template says, and takes its
 * hypervisor and size unless the form sets them: the control plane fills in
 * what the request leaves out (`resolve_create_request` in qarax/src/model/vms.rs).
 */
export function toNewVm(form: VmForm, template: VmTemplate | undefined): Checked {
	const errors: Partial<Record<Field, string>> = {};
	const name = form.name.trim();
	if (!name) errors.name = 'Give the VM a name.';

	const imageRef = form.imageRef.trim();
	if (!template) {
		if (form.boot === 'image' && !imageRef)
			errors.imageRef = 'Enter the image to boot, such as docker.io/library/alpine:latest.';
		if (form.boot === 'disk' && !form.diskId) errors.diskId = 'Pick the disk to boot.';
		if (form.boot === 'disk' && form.hypervisor === 'firecracker')
			errors.hypervisor =
				"Firecracker boots kernels only, so it can't boot a disk through firmware. Pick Cloud Hypervisor.";
	}

	let memoryBytes: number | undefined;
	if (form.size === 'instance-type') {
		if (!form.instanceTypeId) errors.instanceTypeId = 'Pick an instance type.';
	} else if (form.size === 'custom') {
		if (form.vcpus === null || !Number.isInteger(form.vcpus) || form.vcpus < 1)
			errors.vcpus = 'Enter a whole number of vCPUs, 1 or more.';
		memoryBytes = form.memory === null ? NaN : form.memory * UNIT_BYTES[form.memoryUnit];
		if (!(memoryBytes > 0) || !Number.isInteger(memoryBytes / MIB))
			errors.memory = 'Enter a memory size of 1 MiB or more, in whole MiB.';
	}

	if (Object.keys(errors).length) return { kind: 'invalid', errors };

	const tags = [
		...new Set(
			form.tags
				.split(',')
				.map((tag) => tag.trim())
				.filter(Boolean)
		)
	];
	const request: NewVm = {
		name,
		vm_template_id: template?.id ?? null,
		description: form.description.trim() || null,
		tags: tags.length ? tags : null,
		hypervisor: template?.hypervisor ? null : form.hypervisor,
		guest_agent: form.guestAgent || null,
		ha_enabled: form.ha || null,
		network_id: form.networkId || null,
		security_group_ids:
			form.networkId && form.securityGroupIds.length ? form.securityGroupIds : null,
		cloud_init_user_data: form.userData.trim() ? form.userData : null
	};
	if (!template) {
		switch (form.boot) {
			case 'image':
				request.image_ref = imageRef;
				break;
			case 'kernel':
				request.boot_source_id = form.bootSourceId || null;
				break;
			case 'disk':
				request.root_disk_object_id = form.diskId;
				request.boot_mode = 'firmware';
				break;
		}
	}
	switch (form.size) {
		case 'instance-type':
			request.instance_type_id = form.instanceTypeId;
			break;
		case 'custom':
			request.boot_vcpus = form.vcpus;
			request.memory_size = memoryBytes;
			break;
		case 'template':
			break;
	}
	return { kind: 'valid', request };
}
