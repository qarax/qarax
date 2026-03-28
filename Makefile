.PHONY: build test test-deps clean openapi sdk help lint fmt shfmt ruff-check ruff-fmt appliance-build appliance-push run-local stop-local

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
	@echo "  make ruff-check - Run ruff check on Python SDK"
	@echo "  make ruff-fmt   - Run ruff format on Python SDK"
	@echo "  make appliance-build - Build bootc appliance image locally"
	@echo "  make appliance-push  - Push appliance image to registry"
	@echo "  make run-local       - Run full stack locally (qarax + qarax-node + Postgres) via Docker"
	@echo "  make run-local VM=1  - Same, but run qarax-node in a libvirt VM instead of a container"
	@echo "  make stop-local      - Stop and remove the local Docker stack and volumes"

build:
	cargo build --workspace $(CARGO_TARGET)
	cargo run $(CARGO_TARGET) -p qarax --bin generate-openapi

openapi:
	cargo run $(CARGO_TARGET) -p qarax --bin generate-openapi

sdk: openapi
	cd python-sdk && uv run openapi-python-client generate --path ../openapi.yaml --meta setup --overwrite --custom-template-path templates
	cd python-sdk && ruff format .
	cd python-sdk && ruff check --fix .
	cd python-sdk && uvx ty check .

# Database env vars point config to localhost (overrides local.yaml's host: postgres)
# Credentials match both the standalone start_db.sh postgres and the E2E compose postgres.
test: test-deps
	DATABASE_HOST=localhost DATABASE_PORT=5432 \
	DATABASE_USERNAME=qarax DATABASE_PASSWORD=qarax DATABASE_NAME=qarax \
	cargo nextest run $(CARGO_TARGET)

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

shfmt:
	shfmt -w -i 0 hack/*.sh scripts/*.sh

ruff-check:
	@cd python-sdk && ruff check .

ruff-fmt:
	@cd python-sdk && ruff format .

appliance-build:
	cargo build --release -p qarax-node --target $(APPLIANCE_TARGET)
	$(CONTAINER_ENGINE) build \
		-f deployments/Containerfile.qarax-vmm \
		--build-arg CLOUD_HYPERVISOR_VERSION=$(CLOUD_HYPERVISOR_VERSION) \
		--build-arg QARAX_VERSION=$(APPLIANCE_TAG) \
		-t $(APPLIANCE_IMAGE):$(APPLIANCE_TAG) \
		.

appliance-push:
	$(CONTAINER_ENGINE) push $(APPLIANCE_IMAGE):$(APPLIANCE_TAG)

run-local:
	./hack/run-local.sh $(if $(filter 1,$(VM)),--vm)

stop-local:
	./hack/run-local.sh --cleanup
