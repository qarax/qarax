ALTER TABLE hosts
    ADD COLUMN credential_ref TEXT,
    DROP COLUMN password;
