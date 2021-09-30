-- Add migration script here
CREATE TABLE kernels (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    storage_id UUID REFERENCES storage(id) NOT NULL
)

