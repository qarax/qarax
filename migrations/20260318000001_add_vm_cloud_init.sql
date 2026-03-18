ALTER TABLE vms
    ADD COLUMN cloud_init_user_data     TEXT,
    ADD COLUMN cloud_init_meta_data     TEXT,
    ADD COLUMN cloud_init_network_config TEXT;
