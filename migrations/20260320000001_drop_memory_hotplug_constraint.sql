-- Drop the check_memory_hotplug constraint which incorrectly requires
-- memory_hotplug_size >= memory_size. After a memory hotplug operation,
-- memory_size is updated to the new (larger) value, but memory_hotplug_size
-- remains the original additional capacity. The constraint would wrongly
-- reject valid post-resize states.
ALTER TABLE vms DROP CONSTRAINT IF EXISTS check_memory_hotplug;
