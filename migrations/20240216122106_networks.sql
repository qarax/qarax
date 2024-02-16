CREATE TYPE network_mode AS ENUM (
    'STATIC',
    'DHCP',
    'NONE'
);

CREATE TABLE vm_network_interfaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    network_mode network_mode NOT NULL,
    ip_address inet,
    mac_address macaddr UNIQUE NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_vm FOREIGN KEY (vm_id) REFERENCES vms(id)
);