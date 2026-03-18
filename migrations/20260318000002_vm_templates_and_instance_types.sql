CREATE TABLE instance_types (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(100) UNIQUE NOT NULL,
    description TEXT,
    boot_vcpus INTEGER NOT NULL,
    max_vcpus INTEGER NOT NULL,
    cpu_topology JSONB,
    kvm_hyperv BOOLEAN,
    memory_size BIGINT NOT NULL,
    memory_hotplug_size BIGINT,
    memory_mergeable BOOLEAN,
    memory_shared BOOLEAN,
    memory_hugepages BOOLEAN,
    memory_hugepage_size BIGINT,
    memory_prefault BOOLEAN,
    memory_thp BOOLEAN,
    accelerator_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER update_instance_types_modtime
BEFORE UPDATE ON instance_types
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();

CREATE TABLE vm_templates (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(100) UNIQUE NOT NULL,
    description TEXT,
    hypervisor hypervisor,
    boot_vcpus INTEGER,
    max_vcpus INTEGER,
    cpu_topology JSONB,
    kvm_hyperv BOOLEAN,
    memory_size BIGINT,
    memory_hotplug_size BIGINT,
    memory_mergeable BOOLEAN,
    memory_shared BOOLEAN,
    memory_hugepages BOOLEAN,
    memory_hugepage_size BIGINT,
    memory_prefault BOOLEAN,
    memory_thp BOOLEAN,
    boot_source_id UUID REFERENCES boot_sources(id),
    root_disk_object_id UUID REFERENCES storage_objects(id),
    boot_mode boot_mode,
    image_ref TEXT,
    cloud_init_user_data TEXT,
    cloud_init_meta_data TEXT,
    cloud_init_network_config TEXT,
    network_id UUID REFERENCES networks(id),
    networks JSONB,
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_vm_templates_boot_source ON vm_templates(boot_source_id);
CREATE INDEX idx_vm_templates_root_disk ON vm_templates(root_disk_object_id);
CREATE INDEX idx_vm_templates_network ON vm_templates(network_id);

CREATE TRIGGER update_vm_templates_modtime
BEFORE UPDATE ON vm_templates
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();
