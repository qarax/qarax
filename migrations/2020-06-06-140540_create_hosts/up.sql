-- Your SQL goes here
CREATE TABLE hosts (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY NOT NULL,
    name VARCHAR(50) UNIQUE NOT NULL,
    address VARCHAR(16) UNIQUE NOT NULL,
    port INT4 NOT NULL,
	status INT4 NOT NULL,
    host_user VARCHAR(32) NOT NULL,
    password VARCHAR(255) NOT NULL
)
