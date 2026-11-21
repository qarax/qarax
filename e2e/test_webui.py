"""
E2E tests for the web UI: the control-plane image serves its build at /ui, and
the console connection the UI makes reaches a VM's serial console.
"""

import asyncio
import re

import httpx2
import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import force_stop as force_stop_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.models import Hypervisor, NewVm, VmStatus
from websockets.asyncio.client import connect

from helpers import (
    AUTH_HEADERS,
    QARAX_TEST_TOKEN,
    QARAX_URL,
    call_api,
    call_api_detailed,
    wait_for_status,
)

CONSOLE_DEADLINE_SECONDS = 90


def test_ui_is_served():
    """The image ships the UI's build: any page under /ui is its index.html,
    which loads the build's hashed scripts, cached for good."""
    page = httpx2.get(f"{QARAX_URL}/ui/vms")
    assert page.status_code == 200
    assert page.headers["content-type"].startswith("text/html")
    script = re.search(r"/ui/_app/immutable/[^\"']+\.js", page.text)
    assert script, f"index.html names no script: {page.text[:500]}"

    asset = httpx2.get(f"{QARAX_URL}{script.group(0)}")
    assert asset.status_code == 200
    assert "immutable" in asset.headers["cache-control"]


@pytest.mark.asyncio
async def test_console_with_token_subprotocol():
    """The console as the UI attaches to it: a browser can't set the
    Authorization header on a WebSocket, so the token goes as a subprotocol.
    What the guest shell (qarax-init's /bin/sh on the serial console) prints
    for typed input comes back."""
    async with Client(base_url=QARAX_URL, headers=AUTH_HEADERS) as c:
        new_vm = NewVm(
            name="test-webui-console",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        vm_id = str(await call_api(create_vm, client=c, body=new_vm))
        try:
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            url = QARAX_URL.replace("http", "ws", 1) + f"/v1/vms/{vm_id}/console/attach"
            token = QARAX_TEST_TOKEN.encode().hex()
            async with connect(
                url, subprotocols=["qarax.console", f"qarax.bearer.{token}"]
            ) as ws:
                assert ws.subprotocol == "qarax.console"
                # The shell's echo of this line has no 42; only running it does.
                command = b"echo qarax-ui-$((6*7))\r"
                output = b""
                deadline = asyncio.get_running_loop().time() + CONSOLE_DEADLINE_SECONDS
                while b"qarax-ui-42" not in output:
                    assert asyncio.get_running_loop().time() < deadline, (
                        f"the guest shell didn't answer; the console said: {output[-800:]!r}"
                    )
                    # Again until the shell is up to read it.
                    await ws.send(command)
                    try:
                        while b"qarax-ui-42" not in output:
                            frame = await asyncio.wait_for(ws.recv(), timeout=3)
                            # A text frame is the server's: it couldn't attach.
                            assert isinstance(frame, bytes), frame
                            output += frame
                    except TimeoutError:
                        pass
        finally:
            await call_api_detailed(force_stop_vm, client=c, vm_id=vm_id)
            await call_api(delete_vm, client=c, vm_id=vm_id)
