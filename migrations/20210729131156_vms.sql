-- Add migration script here
CREATE TABLE vms (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    status VARCHAR(18) NOT NULL,
    host_id UUID REFERENCES hosts(id),
    vcpu INTEGER NOT NULL,
    memory INTEGER NOT NULL,
    kernel UUID REFERENCES kernels(id) NOT NULL,
    ip_address VARCHAR(16),
    mac_address VARCHAR(20),
    network_mode VARCHAR(10) NOT NULL,
    kernel_params VARCHAR(1000) NOT NULL
)