-- Why the host's last deploy or upgrade failed. Set together with
-- INSTALLATION_FAILED and cleared by every other status change.
ALTER TABLE hosts ADD COLUMN installation_error TEXT;
