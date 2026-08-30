import asyncio
from test_network import call_api, QARAX_URL
from qarax_api_client.api.networks import create as create_network, attach_host
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.models import NewNetwork
from qarax_api_client.models.attach_host_request import AttachHostRequest
from qarax_api_client import Client
from helpers import AUTH_HEADERS
import uuid

async def run():
    import contextlib
    @contextlib.asynccontextmanager
    async def make_client():
        c = Client(base_url=QARAX_URL, headers=AUTH_HEADERS)
        yield c
        
    async with make_client() as c:
        net_id = await call_api(create_network, client=c, body=NewNetwork(name=f"test-db-{uuid.uuid4().hex[:8]}", subnet="10.99.0.0/24", gateway="10.99.0.1", type_="isolated"))
        net_id_str = str(net_id)
        hosts = await call_api(list_hosts, client=c)
        host_id = str(hosts[0].id)
        print("Network:", net_id_str, "Host:", host_id)
        await call_api(attach_host, client=c, network_id=net_id_str, body=AttachHostRequest(host_id=hosts[0].id, bridge_name="br-from-db"))
        print("Attached")

if __name__ == "__main__":
    asyncio.run(run())
