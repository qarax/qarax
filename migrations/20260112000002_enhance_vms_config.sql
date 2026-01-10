-- Enhance VMs table with Cloud Hypervisor CPU and memory configuration

-- Add CPU configuration columns
ALTER TABLE vms
  ADD COLUMN boot_vcpus INTEGER,
  ADD COLUMN max_vcpus INTEGER,
  ADD COLUMN cpu_topology JSONB,
  ADD COLUMN kvm_hyperv BOOLEAN DEFAULT false;

-- Migrate existing vcpu column to boot_vcpus and max_vcpus
UPDATE vms SET boot_vcpus = vcpu, max_vcpus = vcpu WHERE boot_vcpus IS NULL;

-- Make the new CPU columns non-nullable
ALTER TABLE vms ALTER COLUMN boot_vcpus SET NOT NULL;
ALTER TABLE vms ALTER COLUMN max_vcpus SET NOT NULL;

-- Drop old vcpu column
ALTER TABLE vms DROP COLUMN vcpu;

-- Add CPU constraints
ALTER TABLE vms ADD CONSTRAINT check_boot_vcpus_max CHECK (boot_vcpus <= max_vcpus);
ALTER TABLE vms ADD CONSTRAINT check_boot_vcpus_positive CHECK (boot_vcpus > 0);

-- Add memory configuration columns (use BIGINT for bytes)
ALTER TABLE vms
  ADD COLUMN memory_size BIGINT,
  ADD COLUMN memory_hotplug_size BIGINT,
  ADD COLUMN memory_mergeable BOOLEAN DEFAULT false,
  ADD COLUMN memory_shared BOOLEAN DEFAULT false,
  ADD COLUMN memory_hugepages BOOLEAN DEFAULT false,
  ADD COLUMN memory_hugepage_size BIGINT,
  ADD COLUMN memory_prefault BOOLEAN DEFAULT false,
  ADD COLUMN memory_thp BOOLEAN DEFAULT true;

-- Migrate existing memory column (assuming it's in MB, convert to bytes)
UPDATE vms SET memory_size = memory::bigint * 1048576 WHERE memory_size IS NULL;

-- Make memory_size non-nullable
ALTER TABLE vms ALTER COLUMN memory_size SET NOT NULL;

-- Drop old memory column
ALTER TABLE vms DROP COLUMN memory;

-- Add memory constraints
ALTER TABLE vms ADD CONSTRAINT check_memory_size_positive CHECK (memory_size > 0);
ALTER TABLE vms ADD CONSTRAINT check_memory_hotplug CHECK (
  memory_hotplug_size IS NULL OR memory_hotplug_size >= memory_size
);

-- Add firmware support to boot_sources
ALTER TABLE boot_sources
  ADD COLUMN firmware_image_id UUID REFERENCES storage_objects(id);

-- Add indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_vms_cpu_config ON vms(boot_vcpus, max_vcpus);
CREATE INDEX IF NOT EXISTS idx_vms_memory ON vms(memory_size);
CREATE INDEX IF NOT EXISTS idx_vms_memory_shared ON vms(memory_shared) WHERE memory_shared = true;
CREATE INDEX IF NOT EXISTS idx_vms_memory_hugepages ON vms(memory_hugepages) WHERE memory_hugepages = true;
