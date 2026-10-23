-- A persistent write layer must outlive every VM disk that references it.
ALTER TABLE vm_disks
    DROP CONSTRAINT vm_disks_upper_storage_object_id_fkey,
    ADD CONSTRAINT vm_disks_upper_storage_object_id_fkey
        FOREIGN KEY (upper_storage_object_id) REFERENCES storage_objects(id)
        ON DELETE RESTRICT;
