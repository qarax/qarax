.PHONY: build build-release test test-deps clean openapi sdk help lint fmt shfmt shellcheck ruff-check ruff-fmt ci-checks check-fmt check-clippy check-test check-sqlx check-openapi check-sdk check-audit check-shell appliance-build appliance-push run-local stop-local

# On macOS, override default musl target (linker fails cross-compiling from Mac)
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
  CARGO_TARGET := --target $(shell rustc -vV 2>/dev/null | grep '^host:' | cut -d' ' -f2)
else
  CARGO_TARGET :=
endif

CONTAINER_ENGINE ?= docker
APPLIANCE_IMAGE ?= ghcr.io/yourorg/qarax-vmm-host
APPLIANCE_TAG ?= dev
APPLIANCE_TARGET ?= x86_64-unknown-linux-musl
CLOUD_HYPERVISOR_VERSION_FILE ?= versions/cloud-hypervisor-version
CLOUD_HYPERVISOR_VERSION ?= $(shell tr -d '\n' < $(CLOUD_HYPERVISOR_VERSION_FILE))
FIRECRACKER_VERSION_FILE ?= versions/firecracker-version
FIRECRACKER_VERSION ?= $(shell tr -d '\n' < $(FIRECRACKER_VERSION_FILE))

help:
	@echo "Available targets:"
	@echo "  make build      - Build all packages and generate OpenAPI spec"
	@echo "  make openapi    - Generate OpenAPI spec only"
	@echo "  make sdk        - Regenerate Python SDK from OpenAPI"
	@echo "  make test       - Run all tests (requires Postgres, see test-deps)"
	@echo "  make test-deps  - Start Postgres in Docker for tests"
	@echo "  make clean      - Clean build artifacts"
	@echo "  make lint       - Run cargo clippy (lint)"
	@echo "  make fmt        - Run cargo fmt + shfmt (format)"
	@echo "  make ci-checks  - Run every CI check (CI jobs run the check-* targets; needs make test-deps)"
	@echo "  make ruff-check - Run ruff check on Python SDK"
	@echo "  make ruff-fmt   - Run ruff format on Python SDK"
	@echo "  make appliance-build - Build bootc appliance image locally"
	@echo "  make appliance-push  - Push appliance image to registry"
	@echo "  make run-local       - Run full stack locally (qarax + qarax-node + Postgres) via Docker"
	@echo "  make run-local VM=1  - Same, but the node is the appliance image as a bootc microVM"
	@echo "  make stop-local      - Stop and remove the local Docker stack and volumes"

build:
	cargo build --workspace $(CARGO_TARGET)
	cargo run $(CARGO_TARGET) -p qarax --bin generate-openapi

# The release binaries CI hands to the e2e job (musl, with OpenTelemetry).
build-release:
	SQLX_OFFLINE=true cargo build --workspace --release $(OTEL_FEATURES) $(CARGO_TARGET)

openapi:
	cargo run $(CARGO_TARGET) -p qarax --bin generate-openapi

sdk: openapi
	cd python-sdk && uv run openapi-python-client generate --path ../openapi.yaml --meta setup --overwrite --custom-template-path templates
	cd python-sdk && ruff format .
	cd python-sdk && ruff check --fix .
	cd python-sdk && uvx ty check .

# Database env vars point config to localhost (overrides local.yaml's host: postgres)
# Credentials match both the standalone start_db.sh postgres and the E2E compose postgres.
DB_ENV = DATABASE_HOST=localhost DATABASE_PORT=5432 \
	DATABASE_USERNAME=qarax DATABASE_PASSWORD=qarax DATABASE_NAME=qarax
DATABASE_URL ?= postgres://qarax:qarax@localhost:5432/qarax

test: test-deps
	$(DB_ENV) cargo nextest run $(CARGO_TARGET)

# Start Postgres in Docker for integration tests. Run before 'make test' if needed.
# Skip with SKIP_DOCKER=1 if Postgres is already running (e.g. via Docker Compose).
test-deps:
	@if [ -n "$$SKIP_DOCKER" ]; then echo "Skipping (SKIP_DOCKER=1)"; exit 0; fi; \
	if nc -z localhost 5432 2>/dev/null; then \
		echo "Postgres appears to be running on 5432"; \
	elif command -v docker >/dev/null 2>&1; then \
		echo "Starting Postgres..."; \
		./scripts/start_db.sh || { echo "Docker failed. If Postgres is running elsewhere, use: SKIP_DOCKER=1 make test"; exit 1; }; \
	else \
		echo "Postgres required. Start Docker and run 'make test', or run: SKIP_DOCKER=1 make test (if Postgres is already running)"; exit 1; \
	fi

clean:
	cargo clean

# Linting and formatting
lint: ruff-check
	cargo clippy --workspace -- -D warnings

fmt: ruff-fmt shfmt
	cargo fmt

# The CI checks. The GitHub Actions jobs run these targets, so a failing job
# is reproduced by running its target. check-test and check-sqlx need
# Postgres (make test-deps). CI builds with the OpenTelemetry features.
OTEL_FEATURES = --features qarax/otel,qarax-node/otel

ci-checks: check-fmt check-clippy check-test check-sqlx check-openapi check-sdk check-audit check-shell

check-fmt:
	cargo +nightly fmt --all -- --check

check-clippy:
	SQLX_OFFLINE=true cargo clippy --workspace $(OTEL_FEATURES) $(CARGO_TARGET) -- -D warnings

check-test:
	SQLX_OFFLINE=true $(DB_ENV) cargo nextest run --workspace $(OTEL_FEATURES) $(CARGO_TARGET)

# The committed offline query cache (.sqlx) matches the queries in the source.
check-sqlx:
	DATABASE_URL=$(DATABASE_URL) cargo sqlx migrate run
	DATABASE_URL=$(DATABASE_URL) cargo sqlx prepare --workspace --check

# The committed spec and Python SDK are what the source generates.
check-openapi: openapi
	@git diff --exit-code -- openapi.yaml || \
		(echo "openapi.yaml is out of date: run 'make openapi' and commit it"; exit 1)

check-sdk: sdk
	@git diff --exit-code -- python-sdk || \
		(echo "python-sdk is out of date: run 'make sdk' and commit it"; exit 1)

check-audit:
	cargo audit

check-shell:
	shellcheck --severity=warning $(SHELL_SCRIPTS)
	@shfmt -d -i 0 $(SHELL_SCRIPTS) || (echo "shell scripts need formatting: run 'make shfmt'"; exit 1)

# Tracked and new shell scripts that still exist (a deleted file stays in the
# index until it's staged).
SHELL_SCRIPTS = $(shell git ls-files -co --exclude-standard '*.sh' | while read -r f; do [ -f "$$f" ] && echo "$$f"; done)

shfmt:
	shfmt -w -i 0 $(SHELL_SCRIPTS)

shellcheck:
	shellcheck -x $(SHELL_SCRIPTS)

ruff-check:
	@cd python-sdk && ruff check .

ruff-fmt:
	@cd python-sdk && ruff format .

appliance-build:
	cargo build --release -p qarax-node -p qarax-init --target $(APPLIANCE_TARGET)
	$(CONTAINER_ENGINE) build \
		-f deployments/Containerfile.qarax-vmm \
		--build-arg CLOUD_HYPERVISOR_VERSION=$(CLOUD_HYPERVISOR_VERSION) \
		--build-arg FIRECRACKER_VERSION=$(FIRECRACKER_VERSION) \
		--build-arg NODE_ARTIFACTS_SHA=$(shell sha256sum versions/node-artifacts.env | cut -d' ' -f1) \
		--build-arg QARAX_VERSION=$(APPLIANCE_TAG) \
		-t $(APPLIANCE_IMAGE):$(APPLIANCE_TAG) \
		.

appliance-push:
	$(CONTAINER_ENGINE) push $(APPLIANCE_IMAGE):$(APPLIANCE_TAG)

run-local:
	./hack/run-local.sh $(if $(filter 1,$(VM)),--vm)

stop-local:
	./hack/run-local.sh --cleanup
