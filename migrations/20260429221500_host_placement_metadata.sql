ALTER TABLE hosts
    ADD COLUMN IF NOT EXISTS reservation_class TEXT,
    ADD COLUMN IF NOT EXISTS placement_labels JSONB NOT NULL DEFAULT '{}'::jsonb;

CREATE INDEX IF NOT EXISTS idx_hosts_reservation_class
    ON hosts(reservation_class);

CREATE INDEX IF NOT EXISTS idx_hosts_placement_labels
    ON hosts
    USING GIN (placement_labels);
