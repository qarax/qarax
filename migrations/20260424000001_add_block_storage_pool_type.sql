-- Add BLOCK (iSCSI-backed shared block storage) to storage_pool_type enum.
ALTER TYPE storage_pool_type ADD VALUE IF NOT EXISTS 'BLOCK';
