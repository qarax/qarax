#!/usr/bin/env bash
set -eo pipefail

DB_USER=${POSTGRES_USER:=qarax}
DB_PASSWORD="${POSTGRES_PASSWORD:=qarax}"
DB_NAME=${POSTGRES_DB:=qarax}
DB_PORT=${POSTGRES_PORT:=5432}
DB_CONTAINER_NAME=${POSTGRES_CONTAINER_NAME:=qarax-test-postgres}

if [[ -z "${SKIP_DOCKER}" ]]; then
	CONTAINER_ID=$(docker ps -aq --filter "name=^${DB_CONTAINER_NAME}$")
	if [[ -n "${CONTAINER_ID}" ]]; then
		STATUS=$(docker inspect --format '{{.State.Status}}' "${CONTAINER_ID}")
		if [[ "${STATUS}" != "running" ]]; then
			docker start "${CONTAINER_ID}" >/dev/null
		fi
		echo >&2 "Using Postgres container ${DB_CONTAINER_NAME} (${CONTAINER_ID})"
	else
		CONTAINER_ID=$(docker run \
			--name "${DB_CONTAINER_NAME}" \
			--label qarax.role=test-postgres \
			-e POSTGRES_USER="${DB_USER}" \
			-e POSTGRES_PASSWORD="${DB_PASSWORD}" \
			-e POSTGRES_DB="${DB_NAME}" \
			-p "${DB_PORT}":5432 \
			-d docker.io/library/postgres:16)
		echo >&2 "Started Postgres container ${DB_CONTAINER_NAME} (${CONTAINER_ID})"
	fi
	until docker exec "${CONTAINER_ID}" pg_isready -U "${DB_USER}"; do
		echo >&2 "Postgres is still unavailable - sleeping"
		sleep 2
	done
else
	if command -v psql >/dev/null 2>&1; then
		export PGPASSWORD="${DB_PASSWORD}"
		until psql -h "localhost" -U "${DB_USER}" -p "${DB_PORT}" -d "postgres" -c '\q'; do
			echo >&2 "Postgres is still unavailable - sleeping"
			sleep 2
		done
	else
		until nc -z localhost "${DB_PORT}" 2>/dev/null; do
			echo >&2 "Postgres is still unavailable - sleeping"
			sleep 2
		done
	fi
fi

echo >&2 "Postgres is up and running on port ${DB_PORT} - running migrations now!"

export DATABASE_URL=postgres://${DB_USER}:${DB_PASSWORD}@localhost:${DB_PORT}/${DB_NAME}
sqlx database create
echo "Created DB ${DATABASE_URL}"

sqlx mig run
echo "ran migrations"
echo >&2 "Postgres has been migrated, ready to go!"
