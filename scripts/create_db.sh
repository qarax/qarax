#!/bin/bash -e
DB_NAME=${1:-"qarax"}
DB_USER=${2:-"qarax"}
PREFIX="sudo su - postgres -c " 
existing_user=$(${PREFIX} "psql -U postgres -d template1 -c \"SELECT usename FROM pg_catalog.pg_user WHERE usename = '${DB_USER}';\" --tuples-only")

if [ ! -z "$existing_user" ]
then
    echo "User already exists, not creating"
else
    echo "Creating user ${DB_USER}..."
    ${PREFIX} "psql -U postgres -d template1 -c \"create user ${DB_USER} password 'qarax';\""
fi

existing_db=$(${PREFIX} "psql -U postgres -d template1 -c \"SELECT datname FROM pg_database WHERE datname = '${DB_NAME}';\" --tuples-only")
if [ ! -z "$existing_db" ]
then
    echo "Database already exists, not creating"
else
    ${PREFIX} "psql -U postgres -d template1 -c \"create database ${DB_NAME} owner ${DB_USER} template template0
    encoding 'UTF8' lc_collate 'en_US.UTF-8' lc_ctype 'en_US.UTF-8';\""
    ${PREFIX} "psql -U postgres -d "${DB_NAME}" -c \"CREATE EXTENSION \"pgcrypto\";\""
fi

exit 0
