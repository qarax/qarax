table! {
    drives (id) {
        id -> Uuid,
        name -> Varchar,
        status -> Varchar,
        readonly -> Bool,
        rootfs -> Bool,
        storage_id -> Uuid,
    }
}

table! {
    hosts (id) {
        id -> Uuid,
        name -> Varchar,
        address -> Varchar,
        port -> Int4,
        status -> Varchar,
        host_user -> Varchar,
        password -> Varchar,
    }
}

table! {
    kernels (id) {
        id -> Uuid,
        name -> Varchar,
        storage_id -> Uuid,
    }
}

table! {
    storage (id) {
        id -> Uuid,
        name -> Varchar,
        status -> Varchar,
        storage_type -> Varchar,
        config -> Jsonb,
    }
}

table! {
    vm_drives_map (vm_id, drive_id) {
        vm_id -> Uuid,
        drive_id -> Uuid,
    }
}

table! {
    vms (id) {
        id -> Uuid,
        name -> Varchar,
        status -> Int4,
        host_id -> Nullable<Uuid>,
        vcpu -> Int4,
        memory -> Int4,
        ip_address -> Nullable<Varchar>,
        mac_address -> Nullable<Varchar>,
        network_mode -> Varchar,
        kernel_params -> Varchar,
        kernel -> Uuid,
    }
}

joinable!(drives -> storage (storage_id));
joinable!(kernels -> storage (storage_id));
joinable!(vm_drives_map -> drives (drive_id));
joinable!(vm_drives_map -> vms (vm_id));
joinable!(vms -> hosts (host_id));
joinable!(vms -> kernels (kernel));

allow_tables_to_appear_in_same_query!(drives, hosts, kernels, storage, vm_drives_map, vms,);
