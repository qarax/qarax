# qarax

qarax aims to be a [firecracker](https://firecracker-microvm.github.io/) orchestrator, it is currently in its very early stages of development

## Prerequisites

```
postgresql
ansible
rust
```

### Development

#### Get the source

```shell
git clone https://github.com/qarax/qarax.git
cd qarax
```

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

```shell
./e2e/run_tests.sh
```

## FAQ

> Will we succeed?

No