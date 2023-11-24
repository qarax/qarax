CREATE TYPE vm_status AS ENUM (
    'UNKNOWN',
    'DOWN',
    'UP'
);
CREATE TYPE network_mode AS ENUM (
    'STATIC',
    'DHCP',
    'NONE'
);

CREATE TABLE vms (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    status vm_status NOT NULL,
    host_id UUID REFERENCES hosts(id),
    vcpu INTEGER NOT NULL,
    memory INTEGER NOT NULL,
    ip_address inet,
    mac_address macaddr,
    network_mode network_mode NOT NULL,
    kernel_params VARCHAR(1000) NOT NULL,
    kernel UUID NOT NULL
)