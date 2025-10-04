CREATE TYPE interface_type AS ENUM (
    'macvtap'
);

CREATE TABLE networks (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    subnet CIDR NOT NULL,
    type VARCHAR(50),
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE network_interfaces (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    network_id UUID NOT NULL REFERENCES networks(id) ON DELETE CASCADE,
    mac_address MACADDR UNIQUE NOT NULL,
    ip_address INET UNIQUE NOT NULL,
    interface_type interface_type NOT NULL,
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

CREATE TRIGGER update_networks_modtime
BEFORE UPDATE ON networks
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();

CREATE TRIGGER update_network_interfaces_modtime
BEFORE UPDATE ON network_interfaces
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();
