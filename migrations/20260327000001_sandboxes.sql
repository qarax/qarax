CREATE TYPE sandbox_status AS ENUM ('PROVISIONING', 'READY', 'ERROR', 'DESTROYING');

CREATE TABLE sandboxes (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vm_id             UUID NOT NULL UNIQUE REFERENCES vms(id) ON DELETE CASCADE,
    vm_template_id    UUID REFERENCES vm_templates(id) ON DELETE SET NULL,
    name              TEXT NOT NULL UNIQUE,
    status            sandbox_status NOT NULL DEFAULT 'PROVISIONING',
    idle_timeout_secs INT NOT NULL DEFAULT 300,
    last_activity_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    error_message     TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sandboxes_status ON sandboxes (status);
CREATE INDEX idx_sandboxes_last_activity ON sandboxes (last_activity_at) WHERE status = 'READY';
