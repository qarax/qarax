# qarax
qarax aims to be a [firecracker](https://firecracker-microvm.github.io/) orchestrator, it is currently in its very early stages of development

### Prerequisites
```
postgresql
ansible
rust
```
Note: currently Rust nightly is required because of Rocket, use the [following](https://rocket.rs/v0.4/guide/getting-started/#installing-rust) instructions for installation.

### Development

#### Get the source
```shell
$ git clone https://github.com/qarax/qarax.git
$ cd qarax
```

#### Create the database
```shell
$ ./scripts/create_db.sh
```

#### Run the server
```shell
$ cargo run --bin qarax
```

#### Execute tests
```shell
$ ./scripts/run_tests.sh
```
Note: a simple `cargo test` will not work as the tests require a database and cannot be run in parallel.

##### Example requests (will change over time)

Adding a host:
```shell
$ curl -XPOST http://localhost:8000/hosts -d '{ "name":"hosto", "address": "192.168.122.45", "host_user": "root", "password": "fedora", "port": 50051}' -H "Content-Type: application/json"
```
Example reply:
```json
{"id":"365e5061-62b9-41e5-9766-47fcd2c51721"}
```

Installing a host:
```shell
$ curl -XPOST http://localhost:8000/hosts/365e5061-62b9-41e5-9766-47fcd2c51721/install -d '{ "local_node_path": "/path/to/qarax", "fcversion": "v0.21.1" }' -H "Content-Type: application/json"
```

Adding a VM:
```shell
$ curl -XPOST http://localhost:8000/vms -d '{ "name": "hello", "vcpu": 1, "memory": 128, "kernel": "vmlinux", "root_file_system": "rootfs" }' -H "Content-Type: application/json"
```

Start a VM:
```shell
$ curl -XPOST http://localhost:8000/vms/71205bf5-c444-458f-b1b1-918757ee4892/start
```

Stop a VM:
```shell
$ curl -XPOST http://localhost:8000/vms/71205bf5-c444-458f-b1b1-918757ee4892/stop
```

# FAQ
> Will we succeed?

Unclear at the moment. Only time will tell.

