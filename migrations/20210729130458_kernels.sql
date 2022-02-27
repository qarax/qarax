-- Add migration script here
CREATE TABLE kernels (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    volume_id UUID REFERENCES volumes(id) NOT NULL
)

