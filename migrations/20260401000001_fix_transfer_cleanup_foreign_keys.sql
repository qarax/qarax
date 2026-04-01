ALTER TABLE transfers
    DROP CONSTRAINT IF EXISTS transfers_storage_object_id_fkey,
    ADD CONSTRAINT transfers_storage_object_id_fkey
        FOREIGN KEY (storage_object_id)
        REFERENCES storage_objects(id)
        ON DELETE SET NULL;

ALTER TABLE transfers
    DROP CONSTRAINT IF EXISTS transfers_storage_pool_id_fkey,
    ADD CONSTRAINT transfers_storage_pool_id_fkey
        FOREIGN KEY (storage_pool_id)
        REFERENCES storage_pools(id)
        ON DELETE CASCADE;
