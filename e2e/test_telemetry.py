import os

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import init as init_host
from qarax_api_client.api.hosts import list_ as list_hosts

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
QARAX_NODE_HOST = os.getenv("QARAX_NODE_HOST", "qarax-node")


@pytest.mark.telemetry
def test_otel_exports_trace_propagation_and_metrics(telemetry_collector):
    client = Client(base_url=QARAX_URL)
    hosts = list_hosts.sync(client=client)
    assert hosts is not None

    host = next((candidate for candidate in hosts if candidate.address == QARAX_NODE_HOST), hosts[0])
    response = init_host.sync_detailed(host_id=host.id, client=client)
    assert response.status_code.value == 200

    shared_trace_ids = telemetry_collector.wait_for_shared_trace(
        {"qarax", "qarax-node"}, timeout=20.0
    )
    assert shared_trace_ids

    metric_names = telemetry_collector.wait_for_metric_names(
        "qarax",
        {"qarax.monitor.cycle.duration", "qarax.monitor.cycles.total"},
        timeout=20.0,
    )
    assert "qarax.monitor.cycle.duration" in metric_names
    assert "qarax.monitor.cycles.total" in metric_names
