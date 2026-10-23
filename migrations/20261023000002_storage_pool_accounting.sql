-- Keep existing over-capacity pools usable for deletion, but disallow growth.
LOCK TABLE storage_objects IN SHARE ROW EXCLUSIVE MODE;
ALTER TABLE storage_pools DROP CONSTRAINT check_capacity;
UPDATE storage_pools p SET allocated_bytes = (
    SELECT COALESCE(SUM(GREATEST(o.size_bytes, 0)), 0)
    FROM storage_objects o WHERE o.storage_pool_id = p.id
);
ALTER TABLE storage_pools
    ALTER COLUMN allocated_bytes SET NOT NULL,
    ADD CONSTRAINT storage_pool_allocation_nonnegative CHECK (allocated_bytes >= 0);
ALTER TABLE storage_objects
    ADD CONSTRAINT storage_object_size_nonnegative CHECK (size_bytes >= 0) NOT VALID;

CREATE FUNCTION adjust_storage_pool_allocation(pool_id UUID, delta BIGINT)
RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    -- Lock the pool row: concurrent reservations cannot spend the same space.
    UPDATE storage_pools
    SET allocated_bytes = allocated_bytes + delta
    WHERE id = pool_id
      AND (delta <= 0 OR capacity_bytes IS NULL OR allocated_bytes + delta <= capacity_bytes);
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Insufficient storage pool capacity'
            USING ERRCODE = '23514', CONSTRAINT = 'storage_pool_capacity';
    END IF;
END;
$$;

CREATE FUNCTION account_storage_object() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        PERFORM adjust_storage_pool_allocation(NEW.storage_pool_id, NEW.size_bytes);
    ELSIF TG_OP = 'DELETE' THEN
        PERFORM adjust_storage_pool_allocation(OLD.storage_pool_id, -GREATEST(OLD.size_bytes, 0));
    ELSIF NEW.storage_pool_id = OLD.storage_pool_id THEN
        PERFORM adjust_storage_pool_allocation(NEW.storage_pool_id, NEW.size_bytes - GREATEST(OLD.size_bytes, 0));
    ELSE
        -- Consistent lock order also makes moves between pools safe.
        PERFORM id FROM storage_pools
        WHERE id IN (OLD.storage_pool_id, NEW.storage_pool_id) ORDER BY id FOR UPDATE;
        PERFORM adjust_storage_pool_allocation(OLD.storage_pool_id, -GREATEST(OLD.size_bytes, 0));
        PERFORM adjust_storage_pool_allocation(NEW.storage_pool_id, NEW.size_bytes);
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER account_storage_object
AFTER INSERT OR UPDATE OF size_bytes, storage_pool_id OR DELETE ON storage_objects
FOR EACH ROW EXECUTE FUNCTION account_storage_object();
