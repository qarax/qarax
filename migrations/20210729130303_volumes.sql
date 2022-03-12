-- Add migration script here
CREATE TABLE volumes (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    size BIGINT NOT NULL,
    volume_type VARCHAR(18) NOT NULL,
    storage_id UUID REFERENCES storage(id) NOT NULL
)
