-- Enhance vm_disks with Cloud Hypervisor advanced disk configuration

-- Rename device_name to device_path for clarity
ALTER TABLE vm_disks RENAME COLUMN device_name TO device_path;

-- Add disk_id column for Cloud Hypervisor identification
ALTER TABLE vm_disks ADD COLUMN disk_id VARCHAR(50);

-- Generate unique disk IDs for existing disks
UPDATE vm_disks SET disk_id = 'disk-' || id::text WHERE disk_id IS NULL;

-- Make disk_id NOT NULL and unique
ALTER TABLE vm_disks ALTER COLUMN disk_id SET NOT NULL;
ALTER TABLE vm_disks ADD CONSTRAINT vm_disks_disk_id_unique UNIQUE (disk_id);

-- Make storage_object_id nullable (for vhost-user disks that don't use storage objects)
ALTER TABLE vm_disks ALTER COLUMN storage_object_id DROP NOT NULL;

-- Add direct I/O configuration
ALTER TABLE vm_disks
  ADD COLUMN direct BOOLEAN DEFAULT false;

-- Add vhost-user configuration
ALTER TABLE vm_disks
  ADD COLUMN vhost_user BOOLEAN DEFAULT false,
  ADD COLUMN vhost_socket VARCHAR(255);

-- Add performance tuning
ALTER TABLE vm_disks
  ADD COLUMN num_queues INTEGER DEFAULT 1,
  ADD COLUMN queue_size INTEGER DEFAULT 128,
  ADD COLUMN rate_limiter JSONB,
  ADD COLUMN rate_limit_group VARCHAR(100);

-- Add PCI configuration
ALTER TABLE vm_disks
  ADD COLUMN pci_segment INTEGER DEFAULT 0;

-- Add disk metadata
ALTER TABLE vm_disks
  ADD COLUMN serial_number VARCHAR(100);

-- Add constraints
ALTER TABLE vm_disks ADD CONSTRAINT check_disk_path_or_vhost CHECK (
  (vhost_user = true AND vhost_socket IS NOT NULL) OR
  (vhost_user = false AND storage_object_id IS NOT NULL)
);
ALTER TABLE vm_disks ADD CONSTRAINT check_num_queues_positive CHECK (num_queues > 0);
ALTER TABLE vm_disks ADD CONSTRAINT check_queue_size_positive CHECK (queue_size > 0);

-- Add indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_vm_disks_id ON vm_disks(disk_id);
CREATE INDEX IF NOT EXISTS idx_vm_disks_vhost ON vm_disks(vhost_user) WHERE vhost_user = true;
CREATE INDEX IF NOT EXISTS idx_vm_disks_rate_limit_group ON vm_disks(rate_limit_group) WHERE rate_limit_group IS NOT NULL;
