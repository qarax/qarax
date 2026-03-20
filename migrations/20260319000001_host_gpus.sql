CREATE TABLE host_gpus (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    pci_address VARCHAR(20) NOT NULL,
    model VARCHAR(255),
    vendor VARCHAR(100),
    vram_bytes BIGINT,
    iommu_group INTEGER NOT NULL,
    vm_id UUID REFERENCES vms(id) ON DELETE SET NULL,
    discovered_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(host_id, pci_address)
);

CREATE INDEX idx_host_gpus_host ON host_gpus(host_id);
CREATE INDEX idx_host_gpus_available ON host_gpus(host_id) WHERE vm_id IS NULL;
CREATE INDEX idx_host_gpus_vm ON host_gpus(vm_id) WHERE vm_id IS NOT NULL;
