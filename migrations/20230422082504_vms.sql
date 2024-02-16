CREATE TYPE vm_status AS ENUM (
    'UNKNOWN',
    'DOWN',
    'UP'
);

CREATE TABLE vms (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    status vm_status NOT NULL,
    host_id UUID REFERENCES hosts(id),
    vcpu INTEGER NOT NULL,
    memory INTEGER NOT NULL,
    kernel_params VARCHAR(1000) NOT NULL,
    kernel UUID NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE OR REPLACE FUNCTION update_modified_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_vms_modtime
BEFORE UPDATE ON vms
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();