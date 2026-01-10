-- Enhance network interfaces with advanced Cloud Hypervisor networking support

-- Extend interface_type enum to support TAP and vhost-user
ALTER TYPE interface_type ADD VALUE IF NOT EXISTS 'tap';
ALTER TYPE interface_type ADD VALUE IF NOT EXISTS 'vhost_user';

-- Remove UNIQUE constraints on mac_address and ip_address to allow multiple NICs per VM
ALTER TABLE network_interfaces DROP CONSTRAINT IF EXISTS network_interfaces_mac_address_key;
ALTER TABLE network_interfaces DROP CONSTRAINT IF EXISTS network_interfaces_ip_address_key;

-- Add device_id column for unique device identification (required before making it NOT NULL)
ALTER TABLE network_interfaces ADD COLUMN device_id VARCHAR(50);

-- Generate unique device IDs for existing network interfaces
UPDATE network_interfaces SET device_id = 'net-' || id::text WHERE device_id IS NULL;

-- Make device_id NOT NULL and add unique constraint per VM
ALTER TABLE network_interfaces ALTER COLUMN device_id SET NOT NULL;
ALTER TABLE network_interfaces ADD CONSTRAINT network_interfaces_vm_device_unique UNIQUE (vm_id, device_id);

-- Add TAP interface configuration
ALTER TABLE network_interfaces
  ADD COLUMN tap_name VARCHAR(50);

-- Add MAC address configuration (host-side)
ALTER TABLE network_interfaces
  ADD COLUMN host_mac MACADDR;

-- Add network parameters
ALTER TABLE network_interfaces
  ADD COLUMN mtu INTEGER DEFAULT 1500;

-- Add vhost-user configuration
ALTER TABLE network_interfaces
  ADD COLUMN vhost_user BOOLEAN DEFAULT false,
  ADD COLUMN vhost_socket VARCHAR(255),
  ADD COLUMN vhost_mode VARCHAR(20);

-- Add performance tuning
ALTER TABLE network_interfaces
  ADD COLUMN num_queues INTEGER DEFAULT 1,
  ADD COLUMN queue_size INTEGER DEFAULT 256,
  ADD COLUMN rate_limiter JSONB;

-- Add offload features
ALTER TABLE network_interfaces
  ADD COLUMN offload_tso BOOLEAN DEFAULT true,
  ADD COLUMN offload_ufo BOOLEAN DEFAULT true,
  ADD COLUMN offload_csum BOOLEAN DEFAULT true;

-- Add PCI configuration
ALTER TABLE network_interfaces
  ADD COLUMN pci_segment INTEGER DEFAULT 0,
  ADD COLUMN iommu BOOLEAN DEFAULT false;

-- Add constraints
ALTER TABLE network_interfaces ADD CONSTRAINT check_mtu_positive CHECK (mtu > 0);
ALTER TABLE network_interfaces ADD CONSTRAINT check_num_queues_positive CHECK (num_queues > 0);
ALTER TABLE network_interfaces ADD CONSTRAINT check_queue_size_positive CHECK (queue_size > 0);
ALTER TABLE network_interfaces ADD CONSTRAINT check_vhost_socket CHECK (
  (vhost_user = false) OR (vhost_user = true AND vhost_socket IS NOT NULL)
);

-- Add indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_network_interfaces_vm ON network_interfaces(vm_id);
CREATE INDEX IF NOT EXISTS idx_network_interfaces_device ON network_interfaces(device_id);
CREATE INDEX IF NOT EXISTS idx_network_interfaces_vhost ON network_interfaces(vhost_user) WHERE vhost_user = true;
CREATE INDEX IF NOT EXISTS idx_network_interfaces_network ON network_interfaces(network_id);
