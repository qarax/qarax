-- Add migration script here
CREATE TABLE storage (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    status VARCHAR(18) NOT NULL,
    storage_type VARCHAR(18) NOT NULL,
    config JSONB NOT NULL
)
