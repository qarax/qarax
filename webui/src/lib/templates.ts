import type { VmTemplate } from './api/client';
import { bytes } from './format';

/**
 * What a VM made from a template boots, in words. What the template leaves
 * out comes from the control plane's VM defaults: a kernel, and its
 * initramfs, booted directly.
 */
export function templateBoots(
	template: VmTemplate,
	names: { bootSources: Map<string, string>; disks: Map<string, string> }
): string {
	if (template.image_ref) return `the OCI image ${template.image_ref}`;
	if (template.root_disk_object_id)
		return `the disk ${names.disks.get(template.root_disk_object_id) ?? template.root_disk_object_id}`;
	if (template.boot_source_id)
		return `the boot source ${names.bootSources.get(template.boot_source_id) ?? template.boot_source_id}`;
	return "the control plane's default kernel";
}

/** Its vCPUs and memory, when it sets both: a VM needs both from somewhere. */
export function templateSize(template: VmTemplate): string | null {
	if (!template.boot_vcpus || !template.memory_size) return null;
	const vcpus = template.boot_vcpus === 1 ? '1 vCPU' : `${template.boot_vcpus} vCPUs`;
	return `${vcpus}, ${bytes(template.memory_size)}`;
}
