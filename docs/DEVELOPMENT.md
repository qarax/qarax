

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
cargo install sqlx-cli --no-default-features --features postgres
```

#### .env setup (optional)
```
cp .env.sample .env
```
Change the varibale values if needed.

#### Install ansible collections
```
ansible-galaxy collection install -r ./playbooks/requirements.yml
```

#### Start database server
```
sudo systemctl start postgresql
```

#### Run the server

```shell
cargo run --bin qarax
```

> Having issues with buildning or running the project?
1. `rustup target add x86_64-unknown-linux-musl`
2. install `llvsm`
3. install `musl-gcc`


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

> Having issues with running e2e?
1. install `libvirt`
2. clean the databse from old runs using `truncate hosts cascade;`
