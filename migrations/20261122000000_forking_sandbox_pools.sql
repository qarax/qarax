-- A pool that forks keeps one VM of its template booted, its seed, and makes
-- its members by forking the seed instead of booting each one.
ALTER TABLE sandbox_pools ADD COLUMN fork BOOLEAN NOT NULL DEFAULT false;

-- A forking pool's seed is a member of it that is never claimed.
ALTER TABLE sandbox_pool_members ADD COLUMN seed BOOLEAN NOT NULL DEFAULT false;
CREATE UNIQUE INDEX idx_sandbox_pool_members_one_seed
    ON sandbox_pool_members (sandbox_pool_id) WHERE seed;
