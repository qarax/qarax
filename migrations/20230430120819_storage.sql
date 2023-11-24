CREATE TYPE storage_type AS ENUM (
    'SHARED',
    'LOCAL'
);

CREATE TYPE storage_status AS ENUM (
    'SHARED',
    'LOCAL'
);

CREATE TABLE storages (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    storage_type storage_type NOT NULL,
    config JSONB NOT NULL,
    status storage_status NOT NULL
);