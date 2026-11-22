-- A forking pool keeps a seed on each of seed_hosts hosts. The scheduler keeps
-- a pool's seeds on hosts of their own, and the pool manager destroys a second
-- seed on a host, so one seed per pool no longer holds.
ALTER TABLE sandbox_pools
    ADD COLUMN seed_hosts INT NOT NULL DEFAULT 1 CHECK (seed_hosts >= 1);
DROP INDEX idx_sandbox_pool_members_one_seed;
