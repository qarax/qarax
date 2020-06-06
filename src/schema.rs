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
