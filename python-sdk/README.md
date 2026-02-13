# Qarax Python SDK

This directory contains the Python SDK for the Qarax API, automatically generated from the OpenAPI specification.

## Directory Structure

- `qarax-api-client/` - The generated Python SDK package
- `examples/` - Example scripts demonstrating SDK usage
- `.venv/` - Python virtual environment (for development)

## Generating/Regenerating the SDK

The SDK is automatically generated from the OpenAPI specification using `openapi-python-client`.

### Prerequisites

```bash
cd python-sdk
python3 -m venv .venv
source .venv/bin/activate
pip install openapi-python-client
```

### Generate SDK

```bash
# From the qarax root directory, generate the OpenAPI spec
make openapi

# Then generate the Python SDK
cd python-sdk
source .venv/bin/activate
openapi-python-client generate --path ../openapi.yaml --meta setup
```

This will create/update the `qarax-api-client` directory with the latest SDK.

## Installing the SDK

### For Development

```bash
cd qarax-api-client
pip install -e .
```

### For Production

```bash
cd qarax-api-client
pip install .
```

## Usage

See `examples/basic_usage.py` for a complete example.

### Quick Start

```python
from qarax_api_client import Client
from qarax_api_client.api.vms import list_ as list_vms

# Create client
client = Client(base_url="http://localhost:8000")

# List VMs (synchronous)
with client as c:
    vms = list_vms.sync(client=c)
    for vm in vms:
        print(f"VM: {vm.name} - Status: {vm.status}")
```

### Async Usage

```python
import asyncio
from qarax_api_client import Client
from qarax_api_client.api.vms import list_ as list_vms

async def main():
    client = Client(base_url="http://localhost:8000")
    async with client as c:
        vms = await list_vms.asyncio(client=c)
        for vm in vms:
            print(f"VM: {vm.name} - Status: {vm.status}")

asyncio.run(main())
```

## Available API Functions

### Hosts
- `qarax_api_client.api.hosts.list_()` - List all hosts
- `qarax_api_client.api.hosts.add()` - Add a new host

### VMs
- `qarax_api_client.api.vms.list_()` - List all VMs
- `qarax_api_client.api.vms.get()` - Get VM details by ID

**Note:** More VM lifecycle endpoints (create, start, stop, pause, resume, delete) will be added as the API evolves.

## Development Workflow

When adding new endpoints to the Qarax API:

1. Add the endpoint to `qarax/src/handlers/`
2. Annotate with `#[utoipa::path(...)]`
3. Register in `qarax/src/handlers/mod.rs` ApiDoc
4. Regenerate OpenAPI spec: `make openapi`
5. Regenerate Python SDK (see above)
6. Test with examples or E2E tests

## E2E Testing

E2E tests that use this SDK will be located in the repository and will test the full VM lifecycle.
