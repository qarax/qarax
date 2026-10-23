-- NULL means an externally registered object; an empty array is a legacy
-- UUID-named object whose exact creating host is unknown.
ALTER TABLE storage_objects ADD COLUMN managed_host_ids UUID[];
UPDATE storage_objects o SET managed_host_ids = ARRAY[]::UUID[]
FROM storage_pools p
WHERE p.id = o.storage_pool_id AND p.pool_type IN ('LOCAL', 'NFS')
  AND (
    (o.object_type = 'DISK' AND o.config->>'path' =
        CASE WHEN p.pool_type = 'LOCAL' THEN p.config->>'path'
             ELSE '/var/lib/qarax/pools/' || p.id::TEXT END || '/' || o.id::TEXT)
    OR (o.object_type = 'OVERLAYBD_UPPER'
        AND o.config->>'upper_data' =
            CASE WHEN p.pool_type = 'LOCAL' THEN p.config->>'path'
                 ELSE '/var/lib/qarax/pools/' || p.id::TEXT END || '/' || o.id::TEXT || '.upper.data'
        AND o.config->>'upper_index' =
            CASE WHEN p.pool_type = 'LOCAL' THEN p.config->>'path'
                 ELSE '/var/lib/qarax/pools/' || p.id::TEXT END || '/' || o.id::TEXT || '.upper.index')
  );

CREATE TABLE storage_cleanup (
    object_id UUID PRIMARY KEY,
    pool_id UUID NOT NULL REFERENCES storage_pools(id) ON DELETE RESTRICT,
    upper_layer BOOLEAN NOT NULL,
    size_bytes BIGINT NOT NULL,
    host_ids UUID[] NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Retain the allocation after logical deletion until every owned file has
-- been removed. This trigger and the existing accounting trigger run within
-- the same transaction, including all VM foreign-key checks.
CREATE FUNCTION queue_storage_cleanup() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE targets UUID[];
BEGIN
    IF OLD.managed_host_ids IS NOT NULL THEN
        targets := OLD.managed_host_ids;
        IF cardinality(targets) = 0 THEN
            SELECT ARRAY(SELECT host_id FROM host_storage_pools
                         WHERE storage_pool_id = OLD.storage_pool_id) INTO targets;
        END IF;
        INSERT INTO storage_cleanup(object_id, pool_id, upper_layer, size_bytes, host_ids)
        VALUES (OLD.id, OLD.storage_pool_id, OLD.object_type = 'OVERLAYBD_UPPER',
                GREATEST(OLD.size_bytes, 0), targets);
        -- account_storage_object has already released this allocation. Avoid
        -- re-applying the growth check to legacy pools already over capacity.
        UPDATE storage_pools SET allocated_bytes = allocated_bytes + GREATEST(OLD.size_bytes, 0)
        WHERE id = OLD.storage_pool_id;
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER queue_storage_cleanup AFTER DELETE ON storage_objects
FOR EACH ROW EXECUTE FUNCTION queue_storage_cleanup();

CREATE FUNCTION release_storage_cleanup() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM adjust_storage_pool_allocation(OLD.pool_id, -OLD.size_bytes);
    RETURN NULL;
END;
$$;
CREATE TRIGGER release_storage_cleanup AFTER DELETE ON storage_cleanup
FOR EACH ROW EXECUTE FUNCTION release_storage_cleanup();
