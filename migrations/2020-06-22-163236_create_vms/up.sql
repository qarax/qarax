CREATE TABLE vms (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    status INT4 NOT NULL,
    host_id UUID REFERENCES hosts(id),
    vcpu INTEGER NOT NULL,
    memory INTEGER NOT NULL,
    kernel VARCHAR(255) NOT NULL,
    root_file_system VARCHAR(255) NOT NULL,
    ip_address VARCHAR(16),
    mac_address VARCHAR(20),
    network_mode VARCHAR(10) NOT NULL,
    kernel_params VARCHAR(1000) NOT NULL
)