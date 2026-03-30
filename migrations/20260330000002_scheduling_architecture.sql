ALTER TABLE hosts
    ADD COLUMN architecture VARCHAR(16);

ALTER TABLE instance_types
    ADD COLUMN architecture VARCHAR(16);
