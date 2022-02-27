-- Add migration script here
CREATE TABLE drives (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    volume_id UUID REFERENCES storage(id) NOT NULL,
    config JSONB NOT NULL
);