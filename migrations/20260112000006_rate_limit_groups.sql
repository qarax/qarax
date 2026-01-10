-- Create rate_limit_groups table for shared QoS policies

CREATE TABLE rate_limit_groups (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    vm_id UUID REFERENCES vms(id) ON DELETE CASCADE,
    config JSONB NOT NULL,  -- RateLimiterConfig as JSON (bandwidth and ops token buckets)
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITHOUT TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(vm_id, name)
);

-- Add indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_rate_limit_groups_vm ON rate_limit_groups(vm_id);
CREATE INDEX IF NOT EXISTS idx_rate_limit_groups_name ON rate_limit_groups(name);

-- Add trigger for updated_at
CREATE TRIGGER update_rate_limit_groups_modtime
BEFORE UPDATE ON rate_limit_groups
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();
