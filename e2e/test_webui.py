"""
E2E tests for the web UI: the control-plane image serves its build at /ui, the
console connection the UI makes reaches a VM's serial console, and in a real
browser (headless Chromium) the UI signs in, creates, starts and deletes a VM,
creates a sandbox, runs a command in it and deletes it, and creates a network
and a storage pool, attaches each to a host, detaches it and deletes it.
"""

import asyncio
import re
import uuid

import httpx2
import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import force_stop as force_stop_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.models import Hypervisor, NewVm, VmStatus
from playwright.async_api import Page, async_playwright, expect
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
# A start job boots the guest before the VM turns running.
START_DEADLINE_MS = 120_000


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
    What is typed reaches the guest and comes back: the guest's serial tty
    echoes it (qarax-node keeps its end of the PTY raw, so only the guest
    echoes). The test guest's console runs a script, not a shell, so echo is
    what it answers with."""
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
                marker = f"qarax-ui-{uuid.uuid4().hex[:12]}".encode()
                output = b""
                deadline = asyncio.get_running_loop().time() + CONSOLE_DEADLINE_SECONDS
                while marker not in output:
                    assert asyncio.get_running_loop().time() < deadline, (
                        f"the guest didn't echo what was typed; the console said: {output[-800:]!r}"
                    )
                    # Again until the guest's console is open to take it.
                    await ws.send(marker + b"\r")
                    try:
                        while marker not in output:
                            frame = await asyncio.wait_for(ws.recv(), timeout=3)
                            # A text frame is the server's: it couldn't attach.
                            assert isinstance(frame, bytes), frame
                            output += frame
                    except TimeoutError:
                        pass
        finally:
            await call_api_detailed(force_stop_vm, client=c, vm_id=vm_id)
            await call_api(delete_vm, client=c, vm_id=vm_id)


@pytest.fixture
async def page():
    """A signed-in browser page on the UI."""
    async with async_playwright() as playwright:
        browser = await playwright.chromium.launch()
        try:
            # Wide enough for PatternFly to keep the sidebar beside the page.
            page = await browser.new_page(viewport={"width": 1400, "height": 1000})
            await page.goto(f"{QARAX_URL}/ui/")
            await page.get_by_label("API token").fill(QARAX_TEST_TOKEN)
            await page.get_by_role("button", name="Sign in").click()
            await expect(page.get_by_role("heading", name="Overview")).to_be_visible()
            yield page
        finally:
            await browser.close()


async def test_browser_pages_load(page: Page):
    """Each section of the sidebar, and a host's own page, loads what it shows
    from the API, in the browser, without an error."""
    sections = [
        "Virtual machines",
        "Sandboxes",
        "Hosts",
        "Networks",
        "Storage pools",
        "Overview",
    ]
    for section in sections:
        await (
            page.get_by_role("navigation", name="Global")
            .get_by_role("link", name=section)
            .click()
        )
        await expect(page.get_by_role("heading", level=1, name=section)).to_be_visible()
        # Loaded: no "Loading ..." spinner left, and no "Couldn't load ..." alert.
        await expect(
            page.get_by_role("progressbar", name=re.compile("^Loading"))
        ).to_have_count(0)
        await expect(
            page.get_by_role("alert").filter(has_text="Couldn't load")
        ).to_have_count(0)

    nav = page.get_by_role("navigation", name="Global")
    await nav.get_by_role("link", name="Hosts").click()
    host = page.get_by_role("table", name="Hosts").get_by_role("link").first
    name = await host.inner_text()
    await host.click()
    await expect(page.get_by_role("heading", level=1, name=name)).to_be_visible()
    await expect(page.get_by_role("heading", level=2, name="Capacity")).to_be_visible()
    await expect(
        page.get_by_role("alert").filter(has_text="Couldn't load")
    ).to_have_count(0)


async def test_browser_create_start_delete_vm(page: Page):
    """A VM's life through the UI: the create form makes it, its page starts
    it and follows it to running, and Delete, once confirmed, removes it."""
    name = f"test-webui-browser-{uuid.uuid4().hex[:8]}"
    try:
        nav = page.get_by_role("navigation", name="Global")
        await nav.get_by_role("link", name="Virtual machines").click()
        await page.get_by_role("link", name="Create VM").click()

        # By accessible name: a required field's label text also has its asterisk.
        await page.get_by_role("textbox", name="Name", exact=True).fill(name)
        # The control plane's default kernel and initramfs, as the other tests boot.
        await page.get_by_role("radio", name="A kernel").check()
        await page.get_by_role("spinbutton", name="vCPUs").fill("1")
        await page.get_by_role("spinbutton", name="Memory", exact=True).fill("256")
        await page.get_by_role("combobox", name="Memory unit").select_option("MiB")
        await page.get_by_role("button", name="Create", exact=True).click()

        await expect(page).to_have_url(re.compile(r"/ui/vms/[0-9a-f-]{36}$"))
        await expect(page.get_by_role("heading", level=1, name=name)).to_be_visible()
        await expect(page.get_by_text("Created", exact=True)).to_be_visible()

        await page.get_by_role("button", name="Start", exact=True).click()
        await expect(page.get_by_text("Running", exact=True)).to_be_visible(
            timeout=START_DEADLINE_MS
        )

        await page.get_by_role("button", name="Delete", exact=True).click()
        dialog = page.get_by_role("dialog", name=f"Delete {name}?")
        await dialog.get_by_role("button", name="Delete", exact=True).click()
        await expect(page).to_have_url(re.compile(r"/ui/vms$"))
        await expect(page.get_by_role("link", name=name, exact=True)).to_have_count(0)
        assert not vms_named(name), f"{name} is still there after Delete"
    finally:
        for vm in vms_named(name):
            httpx2.delete(f"{QARAX_URL}/v1/vms/{vm['id']}", headers=AUTH_HEADERS)


async def test_browser_sandbox_runs_a_command(page: Page):
    """A sandbox's life through the UI: the create form makes it from a
    template, its page follows it to ready and runs a command in it, showing
    what the command printed, and Delete, once confirmed, removes it."""
    suffix = uuid.uuid4().hex[:8]
    name = f"test-webui-sandbox-{suffix}"
    # The control plane's default kernel and initramfs, whose init answers as
    # the guest agent.
    template = httpx2.post(
        f"{QARAX_URL}/v1/vm-templates",
        json={
            "name": f"test-webui-sandbox-template-{suffix}",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "memory_size": 256 * 1024 * 1024,
        },
        headers=AUTH_HEADERS,
    )
    template.raise_for_status()
    # The new template's id, as text.
    template_id = template.text.strip('"')
    try:
        nav = page.get_by_role("navigation", name="Global")
        await nav.get_by_role("link", name="Sandboxes").click()
        await page.get_by_role("link", name="Create sandbox").click()

        await page.get_by_role("combobox", name="Template").select_option(
            label=f"test-webui-sandbox-template-{suffix}"
        )
        await page.get_by_role("textbox", name="Name", exact=True).fill(name)
        await page.get_by_role("button", name="Create", exact=True).click()

        await expect(page).to_have_url(re.compile(r"/ui/sandboxes/[0-9a-f-]{36}$"))
        await expect(page.get_by_role("heading", level=1, name=name)).to_be_visible()
        await expect(page.get_by_text("Ready", exact=True)).to_be_visible(
            timeout=START_DEADLINE_MS
        )

        command = page.get_by_role("textbox", name="Command")
        await command.fill("echo ui-$((6 * 7)); echo to-stderr >&2; exit 3")
        await page.get_by_role("button", name="Run", exact=True).click()
        # The shell ran it: the arithmetic is done, each stream is shown, and
        # so is the exit code.
        await expect(page.get_by_text("Exit code 3", exact=True)).to_be_visible()
        await expect(page.get_by_text("ui-42", exact=True)).to_be_visible()
        await expect(page.get_by_text("to-stderr", exact=True)).to_be_visible()

        await page.get_by_role("button", name="Delete", exact=True).click()
        dialog = page.get_by_role("dialog", name=f"Delete {name}?")
        await dialog.get_by_role("button", name="Delete", exact=True).click()
        await expect(page).to_have_url(re.compile(r"/ui/sandboxes$"))
        await expect(page.get_by_role("link", name=name, exact=True)).to_have_count(0)
        assert not sandboxes_named(name), f"{name} is still there after Delete"
    finally:
        for sandbox in sandboxes_named(name):
            httpx2.delete(
                f"{QARAX_URL}/v1/sandboxes/{sandbox['id']}", headers=AUTH_HEADERS
            )
        httpx2.delete(
            f"{QARAX_URL}/v1/vm-templates/{template_id}", headers=AUTH_HEADERS
        )


async def test_browser_network_and_storage_pool(page: Page):
    """A network and a local storage pool made from their forms, attached to
    a host from their pages, detached and deleted once confirmed."""
    suffix = uuid.uuid4().hex[:6]
    hosts = httpx2.get(f"{QARAX_URL}/v1/hosts", headers=AUTH_HEADERS).json()["items"]
    host = next(host["name"] for host in hosts if host["status"] == "up")
    network = f"test-webui-net-{suffix}"
    pool = f"test-webui-pool-{suffix}"
    third_octet = int(suffix, 16) % 250
    nav = page.get_by_role("navigation", name="Global")
    try:
        await nav.get_by_role("link", name="Networks").click()
        await page.get_by_role("link", name="Create network").click()
        await page.get_by_role("textbox", name="Name", exact=True).fill(network)
        await page.get_by_role("textbox", name="Subnet").fill(
            f"10.231.{third_octet}.0/24"
        )
        await page.get_by_role("textbox", name="Gateway").fill(
            f"10.231.{third_octet}.1"
        )
        await page.get_by_role("button", name="Create", exact=True).click()
        await expect(page.get_by_role("heading", level=1, name=network)).to_be_visible()

        await page.get_by_role("combobox", name="Host").select_option(label=host)
        # Linux keeps interface names to 15 characters.
        await page.get_by_role("textbox", name="Bridge").fill(f"qbw{suffix}")
        await page.get_by_role("button", name="Attach", exact=True).click()
        hosts_table = page.get_by_role("table", name="Hosts")
        await expect(hosts_table.get_by_role("link", name=host)).to_be_visible()
        await page.get_by_role("button", name=f"Detach from {host}").click()
        await (
            page.get_by_role("dialog", name=f"Detach {network} from {host}?")
            .get_by_role("button", name="Detach", exact=True)
            .click()
        )
        await expect(page.get_by_text("No host carries it yet")).to_be_visible()
        await page.get_by_role("button", name="Delete", exact=True).click()
        await (
            page.get_by_role("dialog", name=f"Delete {network}?")
            .get_by_role("button", name="Delete", exact=True)
            .click()
        )
        await expect(page).to_have_url(re.compile(r"/ui/networks$"))
        assert not named("networks", network), f"{network} is still there after Delete"

        await nav.get_by_role("link", name="Storage pools").click()
        await page.get_by_role("link", name="Create storage pool").click()
        await page.get_by_role("textbox", name="Name", exact=True).fill(pool)
        await page.get_by_role("textbox", name="Directory").fill(
            f"/var/lib/qarax/test-webui-{suffix}"
        )
        await page.get_by_role("button", name="Create", exact=True).click()
        await expect(page.get_by_role("heading", level=1, name=pool)).to_be_visible()

        await page.get_by_role("combobox", name="Host to attach").select_option(
            label=host
        )
        await page.get_by_role("button", name="Attach", exact=True).click()
        await expect(
            page.get_by_role("table", name="Hosts").get_by_role("link", name=host)
        ).to_be_visible()
        await page.get_by_role("button", name=f"Detach from {host}").click()
        await (
            page.get_by_role("dialog", name=f"Detach {pool} from {host}?")
            .get_by_role("button", name="Detach", exact=True)
            .click()
        )
        await expect(page.get_by_text("No host can use it yet.")).to_be_visible()
        await page.get_by_role("button", name="Delete", exact=True).click()
        await (
            page.get_by_role("dialog", name=f"Delete {pool}?")
            .get_by_role("button", name="Delete", exact=True)
            .click()
        )
        await expect(page).to_have_url(re.compile(r"/ui/storage$"))
        assert not named("storage-pools", pool), f"{pool} is still there after Delete"
    finally:
        for leftover in named("networks", network):
            httpx2.delete(
                f"{QARAX_URL}/v1/networks/{leftover['id']}", headers=AUTH_HEADERS
            )
        for leftover in named("storage-pools", pool):
            httpx2.delete(
                f"{QARAX_URL}/v1/storage-pools/{leftover['id']}", headers=AUTH_HEADERS
            )


def named(collection: str, name: str) -> list[dict]:
    """The networks or storage pools of a name: their lists filter by it."""
    response = httpx2.get(
        f"{QARAX_URL}/v1/{collection}", params={"name": name}, headers=AUTH_HEADERS
    )
    response.raise_for_status()
    return [item for item in response.json()["items"] if item["name"] == name]


def sandboxes_named(name: str) -> list[dict]:
    response = httpx2.get(
        f"{QARAX_URL}/v1/sandboxes", params={"limit": 1000}, headers=AUTH_HEADERS
    )
    response.raise_for_status()
    return [s for s in response.json()["items"] if s["name"] == name]


def vms_named(name: str) -> list[dict]:
    response = httpx2.get(
        f"{QARAX_URL}/v1/vms", params={"name": name}, headers=AUTH_HEADERS
    )
    response.raise_for_status()
    return [vm for vm in response.json()["items"] if vm["name"] == name]
