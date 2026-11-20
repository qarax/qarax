-- A storage object a VM made for itself, such as the image object of a VM
-- booted from an OCI image, goes when the VM does, unless something still
-- refers to it (see storage_object_in_use): then it outlives the VM, unowned.
-- NULL for objects that outlive the VMs using them anyway: the images and
-- disks users register or create, persistent OverlayBD layers, committed
-- disks.
ALTER TABLE storage_objects
    ADD COLUMN owner_vm_id UUID REFERENCES vms(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_storage_objects_owner_vm ON storage_objects(owner_vm_id);

-- Whether a row refers to the storage object, so it can't be deleted: every
-- foreign key to storage_objects without an ON DELETE action.
CREATE FUNCTION storage_object_in_use(object_id UUID) RETURNS BOOLEAN
LANGUAGE sql STABLE AS $$
    SELECT EXISTS (SELECT 1 FROM vm_disks
                   WHERE storage_object_id = object_id OR upper_storage_object_id = object_id)
        OR EXISTS (SELECT 1 FROM vm_templates WHERE root_disk_object_id = object_id)
        OR EXISTS (SELECT 1 FROM boot_sources
                   WHERE object_id IN (kernel_image_id, initrd_image_id, firmware_image_id))
        OR EXISTS (SELECT 1 FROM storage_objects WHERE parent_id = object_id)
$$;

-- The image object each VM booted from an OCI image got, named for it.
UPDATE storage_objects so
SET owner_vm_id = v.id
FROM vms v
WHERE so.object_type = 'OCI_IMAGE' AND so.name = 'overlaybd-' || v.id::text;

-- Those of VMs already deleted: nothing but their pool's capacity is theirs.
DELETE FROM storage_objects so
WHERE so.object_type = 'OCI_IMAGE'
  AND so.owner_vm_id IS NULL
  AND so.name ~ '^overlaybd-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
  AND NOT storage_object_in_use(so.id);
