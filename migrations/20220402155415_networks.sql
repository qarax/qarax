-- Add migration script here
CREATE TABLE networks (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    mac_address macaddr,
    ip_address cidr,
    type VARCHAR(10) NOT NULL
)