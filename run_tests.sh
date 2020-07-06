#!/bin/bash -e

current_dir=$(dirname $(readlink -f $0))
DB_NAME="qarax_test"

echo "Executing tests..."
"${current_dir}"/create_db.sh qarax_test qarax > /dev/null 2>&1
RUSTFLAGS=-Awarnings RUST_TEST_THREADS=1 cargo test 2> /dev/null

echo "Finished running tests, cleaning up..."
psql -U postgres -d template1 -c "drop database ${DB_NAME}" > /dev/null 2>&1
exit 0
