"""
E2E test configuration and shared fixtures.

Registers the qarax-node as a host and sets it to UP before running any tests.
This is required for VM scheduling (the control plane picks a host in UP state).
"""

import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest
from opentelemetry.proto.collector.metrics.v1.metrics_service_pb2 import (
    ExportMetricsServiceRequest,
)
from opentelemetry.proto.collector.trace.v1.trace_service_pb2 import (
    ExportTraceServiceRequest,
)
from qarax_api_client import Client
from qarax_api_client.api.hosts import add as add_host
from qarax_api_client.api.hosts import init as init_host
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.models.new_host import NewHost

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
# Address of qarax-node as seen from the qarax control plane (inside docker network)
QARAX_NODE_HOST = os.getenv("QARAX_NODE_HOST", "qarax-node")
QARAX_NODE_PORT = int(os.getenv("QARAX_NODE_PORT", "50051"))
OTEL_TEST_PORT = int(os.getenv("OTEL_TEST_PORT", "14318"))


def _is_truthy(value: str | None) -> bool:
    return (value or "").lower() in {"1", "true", "yes", "on"}


def _resource_attribute(resource, key: str) -> str | None:
    for attribute in resource.attributes:
        if attribute.key != key:
            continue
        which = attribute.value.WhichOneof("value")
        if which == "string_value":
            return attribute.value.string_value
    return None


class _TelemetryStore:
    def __init__(self):
        self._lock = threading.Lock()
        self.reset()

    def reset(self):
        with self._lock:
            self.traces: list[dict[str, str]] = []
            self.metrics: list[dict[str, str]] = []
            self.errors: list[str] = []

    def record_trace_payload(self, payload: bytes):
        request = ExportTraceServiceRequest()
        request.ParseFromString(payload)
        trace_rows = []
        for resource_spans in request.resource_spans:
            service_name = _resource_attribute(resource_spans.resource, "service.name") or "unknown"
            for scope_spans in resource_spans.scope_spans:
                for span in scope_spans.spans:
                    trace_rows.append(
                        {
                            "service_name": service_name,
                            "trace_id": span.trace_id.hex(),
                            "span_id": span.span_id.hex(),
                            "name": span.name,
                        }
                    )
        with self._lock:
            self.traces.extend(trace_rows)

    def record_metric_payload(self, payload: bytes):
        request = ExportMetricsServiceRequest()
        request.ParseFromString(payload)
        metric_rows = []
        for resource_metrics in request.resource_metrics:
            service_name = _resource_attribute(resource_metrics.resource, "service.name") or "unknown"
            for scope_metrics in resource_metrics.scope_metrics:
                for metric in scope_metrics.metrics:
                    metric_rows.append(
                        {
                            "service_name": service_name,
                            "name": metric.name,
                        }
                    )
        with self._lock:
            self.metrics.extend(metric_rows)

    def record_error(self, error: str):
        with self._lock:
            self.errors.append(error)

    def wait_for_shared_trace(self, services: set[str], timeout: float = 20.0) -> set[str]:
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self._lock:
                if self.errors:
                    raise AssertionError(f"Telemetry collector failed: {self.errors[0]}")
                traces = list(self.traces)
            trace_services: dict[str, set[str]] = {}
            for trace in traces:
                trace_services.setdefault(trace["trace_id"], set()).add(trace["service_name"])
            shared = {
                trace_id
                for trace_id, seen_services in trace_services.items()
                if services.issubset(seen_services)
            }
            if shared:
                return shared
            time.sleep(0.25)
        raise AssertionError(f"Did not observe a shared trace for services {sorted(services)}")

    def wait_for_metric_names(
        self, service_name: str, expected_names: set[str], timeout: float = 20.0
    ) -> set[str]:
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self._lock:
                if self.errors:
                    raise AssertionError(f"Telemetry collector failed: {self.errors[0]}")
                metric_names = {
                    metric["name"]
                    for metric in self.metrics
                    if metric["service_name"] == service_name
                }
            if expected_names.issubset(metric_names):
                return metric_names
            time.sleep(0.25)
        raise AssertionError(
            f"Did not observe metrics {sorted(expected_names)} for service {service_name}"
        )


class _TelemetryHandler(BaseHTTPRequestHandler):
    store: _TelemetryStore

    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", "0"))
        payload = self.rfile.read(content_length)
        try:
            if self.path == "/v1/traces":
                self.store.record_trace_payload(payload)
            elif self.path == "/v1/metrics":
                self.store.record_metric_payload(payload)
            else:
                self.send_error(404)
                return
        except Exception as exc:  # surfaced explicitly to the test via store.errors
            self.store.record_error(f"{self.path}: {exc}")
            self.send_error(500, explain=str(exc))
            return

        self.send_response(200)
        self.end_headers()

    def log_message(self, format, *args):
        return


@pytest.fixture(scope="session")
def _telemetry_store():
    if not _is_truthy(os.getenv("ENABLE_OTEL")):
        pytest.skip("Telemetry e2e requires ENABLE_OTEL=1")

    store = _TelemetryStore()
    server = ThreadingHTTPServer(("0.0.0.0", OTEL_TEST_PORT), _TelemetryHandler)
    server.RequestHandlerClass.store = store
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    try:
        yield store
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@pytest.fixture
def telemetry_collector(_telemetry_store: _TelemetryStore) -> _TelemetryStore:
    _telemetry_store.reset()
    return _telemetry_store


@pytest.fixture(scope="session", autouse=True)
def ensure_host_registered():
    """Register the qarax-node host and initialize it before tests run."""
    client = Client(base_url=QARAX_URL)

    hosts = list_hosts.sync(client=client)
    if hosts is None:
        raise RuntimeError("Failed to list hosts")

    selected_host = next((h for h in hosts if h.address == QARAX_NODE_HOST), None)
    host_id = selected_host.id if selected_host is not None else None

    if host_id is None:
        # Not registered yet — register it now
        new_host = NewHost(
            name="e2e-node",
            address=QARAX_NODE_HOST,
            port=QARAX_NODE_PORT,
            host_user="root",
            password="",
        )
        result = add_host.sync_detailed(client=client, body=new_host)
        if result.status_code.value == 201:
            host_id = uuid.UUID(result.parsed.strip())
        else:
            # Could be a 409/422/500 due to stale DB state; re-fetch to find it
            hosts = list_hosts.sync(client=client)
            if hosts is None:
                raise RuntimeError("Failed to list hosts after registration attempt")
            selected_host = next((h for h in hosts if h.address == QARAX_NODE_HOST), None)
            host_id = selected_host.id if selected_host is not None else None

    if host_id is None:
        raise RuntimeError(
            f"Could not register or find a host at {QARAX_NODE_HOST}:{QARAX_NODE_PORT}"
        )

    # Initialize the selected host so the scheduler sees a reachable UP host.
    result = init_host.sync_detailed(host_id=host_id, client=client)
    if result.status_code.value != 200:
        raise RuntimeError(f"Failed to initialize host {host_id}: HTTP {result.status_code}")
