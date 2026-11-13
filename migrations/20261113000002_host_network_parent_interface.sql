-- The uplink a bridged network uses on each host (NULL for isolated/NAT
-- networks), so host init can re-create the attachment after a node restart.
ALTER TABLE host_networks ADD COLUMN parent_interface TEXT;
