-- Create tables for VM console and RNG devices

-- Create console_mode enum
CREATE TYPE console_mode AS ENUM (
    'OFF',
    'PTY',
    'TTY',
    'FILE',
    'SOCKET',
    'NULL'
);

-- Console configuration table (serial and console devices)
CREATE TABLE vm_consoles (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    console_type VARCHAR(20) NOT NULL,  -- 'SERIAL' or 'CONSOLE'
    mode console_mode NOT NULL,
    file_path VARCHAR(255),
    socket_path VARCHAR(255),
    iommu BOOLEAN DEFAULT false,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(vm_id, console_type)
);

-- RNG configuration table
CREATE TABLE vm_rng (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    src VARCHAR(255) NOT NULL DEFAULT '/dev/urandom',
    iommu BOOLEAN DEFAULT false,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(vm_id)
);

-- Add indexes
CREATE INDEX IF NOT EXISTS idx_vm_consoles_vm ON vm_consoles(vm_id);
CREATE INDEX IF NOT EXISTS idx_vm_consoles_type ON vm_consoles(console_type);
CREATE INDEX IF NOT EXISTS idx_vm_rng_vm ON vm_rng(vm_id);

-- Add triggers for updated_at
CREATE TRIGGER update_vm_consoles_modtime
BEFORE UPDATE ON vm_consoles
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();

CREATE TRIGGER update_vm_rng_modtime
BEFORE UPDATE ON vm_rng
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();
