

#### Get the source

```shell
git clone https://github.com/qarax/qarax.git
cd qarax
```

#### install Prerequisites

```
postgresql
ansible
rust
```

#### Install sqlx
[SQLx CLI](https://github.com/launchbadge/sqlx/tree/master/sqlx-cli)
```
# only for postgres
$ cargo install sqlx-cli --no-default-features --features postgres
```

#### .env setup (optional)
```
$ cp .env.sample .env
```
Current required values are:
```
DATABASE_URL
LOCAL_NODE_PATH
SSH_PUB_KEY
```

#### Install ansible collections
```
$ ansible-galaxy collection install -r ./playbooks/requirements.yml
```

#### Start database server
```
$ sudo systemctl start postgresql
```

### Compile dependencies

This project compiles for the musl target rather than glibc, thus you install the target:

```shell
rustup target add x86_64-unknown-linux-musl
```

Transitive dependencies (namely `ring`) require `musl-gcc` (or your distro's equivalent). 

#### Run the server

```shell
cargo run --bin qarax
```

#### Execute tests

```shell
cargo test -- --test-threads 1
```

Note: a simple `cargo test` will not work as the tests require a database and cannot be run in parallel.

#### e2e tests

Install [Terraform](https://learn.hashicorp.com/tutorials/terraform/install-cli?in=terraform/aws-get-started)

```shell
./e2e/run_tests.sh
```

Running e2e requires `libvirt`, as the terraform plan uses the `libvirt` provides. `libvirtd` must be running. 
