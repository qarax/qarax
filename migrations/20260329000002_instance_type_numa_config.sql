-- Optional NUMA configuration for instance types
-- Shape: { "numa_node": 0 }
ALTER TABLE instance_types ADD COLUMN numa_config JSONB;
