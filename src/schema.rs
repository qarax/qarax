table! {
    hosts (id) {
        id -> Uuid,
        name -> Varchar,
        address -> Varchar,
        port -> Int4,
        status -> Int4,
        host_user -> Varchar,
        password -> Varchar,
    }
}

table! {
    vms (id) {
        id -> Uuid,
        name -> Nullable<Varchar>,
        status -> Nullable<Int4>,
        host_id -> Nullable<Uuid>,
        vcpu -> Nullable<Int4>,
        memory -> Nullable<Int4>,
        kernel -> Nullable<Varchar>,
        root_file_system -> Nullable<Varchar>,
    }
}

joinable!(vms -> hosts (host_id));

allow_tables_to_appear_in_same_query!(
    hosts,
    vms,
);
