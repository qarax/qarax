CREATE TYPE snapshot_status AS ENUM ('CREATING', 'READY', 'FAILED');

CREATE TABLE vm_snapshots (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vm_id         UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    status        snapshot_status NOT NULL DEFAULT 'CREATING',
    snapshot_url  TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX vm_snapshots_vm_id_idx ON vm_snapshots(vm_id);
