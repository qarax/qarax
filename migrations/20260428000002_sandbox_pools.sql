ALTER TYPE job_type ADD VALUE IF NOT EXISTS 'SANDBOX_CLAIM';

CREATE TYPE sandbox_pool_member_status AS ENUM ('PROVISIONING', 'READY', 'ERROR', 'DESTROYING');

CREATE TABLE IF NOT EXISTS sandbox_pools (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vm_template_id UUID NOT NULL UNIQUE REFERENCES vm_templates(id),
    min_ready      INT NOT NULL DEFAULT 0 CHECK (min_ready >= 0),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS sandbox_pool_members (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sandbox_pool_id UUID NOT NULL REFERENCES sandbox_pools(id) ON DELETE CASCADE,
    vm_id           UUID NOT NULL UNIQUE REFERENCES vms(id) ON DELETE CASCADE,
    status          sandbox_pool_member_status NOT NULL DEFAULT 'PROVISIONING',
    error_message   TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sandbox_pool_members_pool_status
    ON sandbox_pool_members (sandbox_pool_id, status);
