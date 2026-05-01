ALTER TYPE storage_object_type ADD VALUE IF NOT EXISTS 'DATABASE_BACKUP';

DO $$
BEGIN
    CREATE TYPE backup_type AS ENUM ('VM', 'DATABASE');
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    CREATE TYPE backup_status AS ENUM ('CREATING', 'READY', 'FAILED');
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS backups (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name              TEXT NOT NULL,
    backup_type       backup_type NOT NULL,
    status            backup_status NOT NULL DEFAULT 'CREATING',
    vm_id             UUID REFERENCES vms(id) ON DELETE CASCADE,
    snapshot_id       UUID UNIQUE REFERENCES vm_snapshots(id) ON DELETE CASCADE,
    storage_object_id UUID NOT NULL UNIQUE REFERENCES storage_objects(id) ON DELETE CASCADE,
    error_message     TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT backups_target_check CHECK (
        (backup_type = 'VM' AND vm_id IS NOT NULL AND snapshot_id IS NOT NULL)
        OR
        (backup_type = 'DATABASE' AND vm_id IS NULL AND snapshot_id IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS backups_type_idx ON backups(backup_type);
CREATE INDEX IF NOT EXISTS backups_vm_id_idx ON backups(vm_id);
CREATE INDEX IF NOT EXISTS backups_created_at_idx ON backups(created_at DESC);

DROP TRIGGER IF EXISTS update_backups_modtime ON backups;
CREATE TRIGGER update_backups_modtime
BEFORE UPDATE ON backups
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();
