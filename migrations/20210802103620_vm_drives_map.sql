-- Add migration script here
CREATE TABLE vm_drives_map (
    vm_id UUID REFERENCES vms(id) NOT NULL,
    drive_id UUID REFERENCES drives(id) NOT NULL,
    PRIMARY KEY(vm_id, drive_id)
);

