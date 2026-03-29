-- Add OVERLAYBD_UPPER variant to storage_object_type enum.
-- This type tracks the persistent writable upper layer (upper.data + upper.index)
-- for linked-persistent OverlayBD VMs.
ALTER TYPE storage_object_type ADD VALUE IF NOT EXISTS 'OVERLAYBD_UPPER';

-- Add an optional reference from vm_disks to an OverlaybdUpper storage object.
-- NULL  => ephemeral upper layer (deleted on VM stop, current behaviour).
-- non-NULL => persistent upper layer stored on a Local or NFS pool.
-- ON DELETE SET NULL: if the StorageObject is explicitly deleted, the disk record
-- loses its reference but is not itself removed.
ALTER TABLE vm_disks
    ADD COLUMN upper_storage_object_id UUID
        REFERENCES storage_objects(id) ON DELETE SET NULL;
