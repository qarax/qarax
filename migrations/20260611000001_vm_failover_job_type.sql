-- Job type for automatic HA failover of VMs from failed hosts.
ALTER TYPE job_type ADD VALUE IF NOT EXISTS 'VM_FAILOVER';
