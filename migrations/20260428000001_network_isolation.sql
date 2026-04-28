CREATE TYPE security_group_direction AS ENUM ('INGRESS', 'EGRESS');
CREATE TYPE security_group_protocol AS ENUM ('ANY', 'TCP', 'UDP', 'ICMP');

ALTER TABLE networks
ADD COLUMN vpc_name VARCHAR(100);

CREATE INDEX IF NOT EXISTS idx_networks_vpc_name ON networks(vpc_name) WHERE vpc_name IS NOT NULL;

CREATE TABLE IF NOT EXISTS security_groups (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        VARCHAR(100) UNIQUE NOT NULL,
    description TEXT,
    created_at  TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER update_security_groups_modtime
BEFORE UPDATE ON security_groups
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();

CREATE TABLE IF NOT EXISTS security_group_rules (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    security_group_id UUID NOT NULL REFERENCES security_groups(id) ON DELETE CASCADE,
    direction         security_group_direction NOT NULL,
    protocol          security_group_protocol NOT NULL DEFAULT 'ANY',
    cidr              CIDR,
    port_start        INTEGER,
    port_end          INTEGER,
    description       TEXT,
    created_at        TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT check_security_group_rule_ports_positive CHECK (
        port_start IS NULL OR (port_start > 0 AND port_start <= 65535)
    ),
    CONSTRAINT check_security_group_rule_port_range CHECK (
        port_end IS NULL OR (port_end > 0 AND port_end <= 65535 AND port_end >= COALESCE(port_start, port_end))
    ),
    CONSTRAINT check_security_group_rule_ports_pair CHECK (
        (port_start IS NULL AND port_end IS NULL) OR (port_start IS NOT NULL AND port_end IS NOT NULL)
    ),
    CONSTRAINT check_security_group_rule_protocol_ports CHECK (
        protocol IN ('TCP', 'UDP') OR (port_start IS NULL AND port_end IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_security_group_rules_group ON security_group_rules(security_group_id);

CREATE TABLE IF NOT EXISTS vm_security_groups (
    vm_id              UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    security_group_id  UUID NOT NULL REFERENCES security_groups(id) ON DELETE CASCADE,
    PRIMARY KEY (vm_id, security_group_id)
);

CREATE INDEX IF NOT EXISTS idx_vm_security_groups_vm ON vm_security_groups(vm_id);
CREATE INDEX IF NOT EXISTS idx_vm_security_groups_group ON vm_security_groups(security_group_id);
