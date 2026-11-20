-- A fork's memory maps its source's memory image copy-on-write: it holds
-- only the pages it has written, which its node measures. The scheduler
-- charges such a VM for those plus headroom, not for its memory size, until
-- it moves on to a hypervisor process of its own (started again, restored,
-- migrated or failed over).
ALTER TABLE vms
    ADD COLUMN memory_copy_on_write BOOLEAN NOT NULL DEFAULT false,
    -- The memory the VM's hypervisor process holds of its own, as its node
    -- last measured it: anonymous, shared memory and huge pages, not the
    -- file pages a fork maps of its source's memory image.
    ADD COLUMN memory_private_bytes BIGINT;

-- What a VM is charged of its host's memory: its memory size, or for a VM
-- whose memory is copy-on-write, the memory it holds of its own plus
-- headroom_ratio of its size for what it writes before the next measurement,
-- never more than its size.
CREATE FUNCTION vm_memory_charge(
    memory_size BIGINT,
    memory_copy_on_write BOOLEAN,
    memory_private_bytes BIGINT,
    headroom_ratio DOUBLE PRECISION
) RETURNS BIGINT
LANGUAGE SQL IMMUTABLE
AS $$
    SELECT CASE
        WHEN memory_copy_on_write THEN LEAST(
            memory_size,
            COALESCE(memory_private_bytes, 0) + (memory_size * headroom_ratio)::BIGINT
        )
        ELSE memory_size
    END
$$;
