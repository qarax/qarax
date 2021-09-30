-- Add migration script here
CREATE TABLE drives (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    status VARCHAR(18) NOT NULL,
    cache_type VARCHAR(18) NOT NULL,
    readonly boolean NOT NULL,
    rootfs boolean NOT NULL,
    storage_id UUID REFERENCES storage(id) NOT NULL
);