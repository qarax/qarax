-- Add migration script here
CREATE TABLE drives (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    volume_id UUID REFERENCES volumes(id) NOT NULL,
    status VARCHAR(18) NOT NULL,
    config JSONB NOT NULL
);