CREATE TABLE vms (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE,
    status INT4,
    host_id UUID REFERENCES hosts(id),
    vcpu INTEGER,
    memory INTEGER,
    kernel VARCHAR(255),
    root_file_system VARCHAR(255)
)