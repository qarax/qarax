CREATE TYPE host_status AS ENUM (
    'UNKNOWN',
    'DOWN',
    'INSTALLING',
    'INSTALLATION_FAILED',
    'INITIALIZING',
    'UP'
);

CREATE TABLE hosts (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY NOT NULL,
    name VARCHAR(50) UNIQUE NOT NULL,
    address VARCHAR(16) UNIQUE NOT NULL,
    port INT4 NOT NULL,
    status host_status NOT NULL,
    host_user VARCHAR(32) NOT NULL,
    password BYTEA NOT NULL
);