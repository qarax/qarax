-- Update VM status enum to align with Cloud Hypervisor lifecycle states
ALTER TYPE vm_status RENAME TO vm_status_old;

CREATE TYPE vm_status AS ENUM (
    'UNKNOWN',    -- Unknown state (legacy/error)
    'CREATED',    -- VM created, not started
    'RUNNING',    -- VM is running
    'PAUSED',     -- VM is paused
    'SHUTDOWN'    -- VM has shut down
);

-- Migrate existing data
ALTER TABLE vms ALTER COLUMN status TYPE vm_status
  USING (CASE status::text
    WHEN 'UP' THEN 'RUNNING'::vm_status
    WHEN 'DOWN' THEN 'SHUTDOWN'::vm_status
    ELSE 'UNKNOWN'::vm_status
  END);

DROP TYPE vm_status_old;
