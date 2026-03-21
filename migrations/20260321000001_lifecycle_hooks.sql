-- Lifecycle hooks: webhook notifications on VM state transitions

CREATE TYPE hook_scope AS ENUM ('GLOBAL', 'VM', 'TAG');

CREATE TYPE hook_execution_status AS ENUM ('PENDING', 'DELIVERED', 'FAILED');

CREATE TABLE lifecycle_hooks (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    url         TEXT NOT NULL,
    secret      TEXT,
    scope       hook_scope NOT NULL DEFAULT 'GLOBAL',
    scope_value TEXT,
    events      TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT lifecycle_hooks_name_key UNIQUE (name)
);

CREATE TABLE hook_executions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    hook_id         UUID NOT NULL REFERENCES lifecycle_hooks(id) ON DELETE CASCADE,
    vm_id           UUID NOT NULL,
    previous_status TEXT NOT NULL,
    new_status      TEXT NOT NULL,
    status          hook_execution_status NOT NULL DEFAULT 'PENDING',
    attempt_count   INT NOT NULL DEFAULT 0,
    max_attempts    INT NOT NULL DEFAULT 5,
    next_retry_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload         JSONB NOT NULL,
    response_status INT,
    response_body   TEXT,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at    TIMESTAMPTZ
);

CREATE INDEX idx_hook_executions_pending
    ON hook_executions (next_retry_at)
    WHERE status = 'PENDING';

CREATE INDEX idx_lifecycle_hooks_scope
    ON lifecycle_hooks (scope, scope_value)
    WHERE active = TRUE;
