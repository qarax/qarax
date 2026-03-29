-- NUMA node topology discovered per host
CREATE TABLE host_numa_nodes (
    id           UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    host_id      UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    node_id      INTEGER NOT NULL,
    cpu_list     TEXT NOT NULL DEFAULT '',
    memory_bytes BIGINT,
    distances    INTEGER[] NOT NULL DEFAULT '{}',
    updated_at   TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (host_id, node_id)
);

CREATE INDEX idx_host_numa_nodes_host ON host_numa_nodes (host_id);

-- NUMA node affinity for discovered GPUs (-1 = unknown)
ALTER TABLE host_gpus ADD COLUMN numa_node INTEGER NOT NULL DEFAULT -1;
