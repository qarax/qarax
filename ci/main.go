// Package main is a Dagger module for the Qarax CI pipeline.
// Call individual steps with: dagger call --mod ./ci <function> --src=.
// Run all checks in parallel:  dagger call --mod ./ci all --src=.
package main

import (
	"context"
	"fmt"
	"strings"

	"dagger/ci/internal/dagger"

	"golang.org/x/sync/errgroup"
)

const (
	otelFeatures = "qarax/otel,qarax-node/otel"
	dbUser       = "postgres"
	dbPassword   = "password"
	dbName       = "qarax"
	dbURL        = "postgres://postgres:password@postgres:5432/qarax"
)

// Ci is the Qarax CI pipeline Dagger module.
type Ci struct{}

func New() *Ci { return &Ci{} }

// postgres returns a Postgres 16 service ready to accept connections.
func (c *Ci) postgres() *dagger.Service {
	return dag.Container().
		From("postgres:16").
		WithEnvVariable("POSTGRES_USER", dbUser).
		WithEnvVariable("POSTGRES_PASSWORD", dbPassword).
		WithEnvVariable("POSTGRES_DB", dbName).
		WithExposedPort(5432).
		AsService()
}

// rustBase returns a stable Rust container with the musl target and build tools,
// the source directory mounted at /src, and Cargo/build caches attached.
func (c *Ci) rustBase(src *dagger.Directory) *dagger.Container {
	return dag.Container().
		From("rust:1").
		WithExec([]string{"sh", "-c",
			"apt-get update && apt-get install -y --no-install-recommends musl-tools protobuf-compiler libprotobuf-dev"}).
		WithExec([]string{"rustup", "target", "add", "x86_64-unknown-linux-musl"}).
		WithMountedCache("/usr/local/cargo/registry", dag.CacheVolume("cargo-registry")).
		WithMountedCache("/usr/local/cargo/git", dag.CacheVolume("cargo-git"), dagger.ContainerWithMountedCacheOpts{Sharing: dagger.CacheSharingModeLocked}).
		WithMountedCache("/src/target", dag.CacheVolume("cargo-target")).
		WithEnvVariable("CARGO_NET_GIT_FETCH_WITH_CLI", "true").
		WithDirectory("/src", src).
		WithWorkdir("/src")
}

// prebuilt compiles the full workspace once in debug mode so that Lint, Test,
// SqlxCheck, and openApiSpec can run without recompiling. All those steps chain
// from this container; Dagger's operation graph deduplicates it so the build
// runs exactly once per session regardless of how many steps reference it.
func (c *Ci) prebuilt(src *dagger.Directory) *dagger.Container {
	return c.rustBase(src).
		WithExec([]string{"rustup", "component", "add", "clippy"}).
		WithEnvVariable("SQLX_OFFLINE", "true").
		WithExec([]string{"cargo", "build", "--workspace", "--features", otelFeatures})
}

// Fmt checks formatting with cargo +nightly fmt --all -- --check.
func (c *Ci) Fmt(ctx context.Context, src *dagger.Directory) (string, error) {
	return dag.Container().
		From("rust:latest").
		WithExec([]string{"rustup", "install", "nightly", "--profile", "minimal"}).
		WithExec([]string{"rustup", "component", "add", "rustfmt", "--toolchain", "nightly"}).
		WithMountedCache("/usr/local/cargo/registry", dag.CacheVolume("cargo-registry")).
		WithDirectory("/src", src).
		WithWorkdir("/src").
		WithExec([]string{"cargo", "+nightly", "fmt", "--all", "--", "--check"}).
		Stdout(ctx)
}

// Lint runs cargo clippy -D warnings against pre-compiled artifacts.
func (c *Ci) Lint(ctx context.Context, src *dagger.Directory) (string, error) {
	return c.prebuilt(src).
		WithExec([]string{"cargo", "clippy", "--workspace",
			"--features", otelFeatures, "--", "-D", "warnings"}).
		Stdout(ctx)
}

// Build compiles the workspace in release mode and returns a directory containing
// the four service binaries: qarax-server, qarax-node, qarax-init, qarax.
func (c *Ci) Build(ctx context.Context, src *dagger.Directory) (*dagger.Directory, error) {
	// /src/target is a cache volume so cannot be snapshotted directly.
	// Copy the four binaries to /out (a plain layer) before returning.
	return c.rustBase(src).
		WithEnvVariable("SQLX_OFFLINE", "true").
		WithExec([]string{"cargo", "build", "--workspace", "--release",
			"--features", otelFeatures}).
		WithExec([]string{"bash", "-c",
			"mkdir /out && cp /src/target/x86_64-unknown-linux-musl/release/{qarax-server,qarax-node,qarax-init,qarax} /out/"}).
		Directory("/out"), nil
}

// Test runs the nextest suite against a live Postgres 16 service using pre-compiled artifacts.
func (c *Ci) Test(ctx context.Context, src *dagger.Directory) (string, error) {
	pg := c.postgres()

	return c.prebuilt(src).
		// Install nextest via binary download — faster than building from source.
		WithExec([]string{"sh", "-c",
			"curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C /usr/local/cargo/bin"}).
		WithServiceBinding("postgres", pg).
		WithEnvVariable("DATABASE_URL", dbURL).
		WithEnvVariable("TEST_DATABASE_URL", dbURL).
		WithEnvVariable("DATABASE_HOST", "postgres").
		WithEnvVariable("DATABASE_PORT", "5432").
		WithEnvVariable("DATABASE_USERNAME", dbUser).
		WithEnvVariable("DATABASE_PASSWORD", dbPassword).
		WithEnvVariable("DATABASE_NAME", dbName).
		WithExec([]string{"cargo", "nextest", "run", "--workspace",
			"--features", otelFeatures}).
		Stdout(ctx)
}

// installBinstall installs cargo-binstall on the given container using the
// official release script, enabling fast prebuilt-binary installs.
func installBinstall(ctr *dagger.Container) *dagger.Container {
	return ctr.WithExec([]string{"sh", "-c",
		"curl -L --proto '=https' --tlsv1.2 -sSf " +
			"https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash"})
}

// SqlxCheck verifies that the committed .sqlx offline query cache matches the
// actual queries in source by running cargo sqlx prepare --workspace --check
// against a live Postgres instance.
func (c *Ci) SqlxCheck(ctx context.Context, src *dagger.Directory) (string, error) {
	pg := c.postgres()
	sqlxBin := installBinstall(dag.Container().From("rust:1")).
		WithExec([]string{"cargo", "binstall", "--no-confirm", "sqlx-cli"}).
		File("/usr/local/cargo/bin/cargo-sqlx")
	return c.prebuilt(src).
		WithFile("/usr/local/cargo/bin/cargo-sqlx", sqlxBin).
		WithServiceBinding("postgres", pg).
		WithoutEnvVariable("SQLX_OFFLINE").
		WithEnvVariable("DATABASE_URL", dbURL).
		WithExec([]string{"cargo", "sqlx", "migrate", "run"}).
		WithExec([]string{"cargo", "sqlx", "prepare", "--workspace", "--check"}).
		Stdout(ctx)
}

// Audit runs cargo audit against Cargo.lock to detect known CVEs.
// Ignores are configured in .cargo/audit.toml at the workspace root.
func (c *Ci) Audit(ctx context.Context, src *dagger.Directory) (string, error) {
	return installBinstall(dag.Container().From("rust:1")).
		WithExec([]string{"cargo", "binstall", "--no-confirm", "cargo-audit"}).
		WithMountedCache("/usr/local/cargo/registry", dag.CacheVolume("cargo-registry")).
		WithDirectory("/src", src, dagger.ContainerWithDirectoryOpts{
			Include: []string{"Cargo.lock", ".cargo/audit.toml"},
		}).
		WithWorkdir("/src").
		WithExec([]string{"cargo", "audit"}).
		Stdout(ctx)
}

// openApiSpec generates openapi.yaml from source and returns it as a File.
// Chains from prebuilt so the binary is already compiled.
func (c *Ci) openApiSpec(src *dagger.Directory) *dagger.File {
	return c.prebuilt(src).
		WithExec([]string{"cargo", "run", "-p", "qarax", "--bin", "generate-openapi"}).
		File("/src/openapi.yaml")
}

// OpenApiCheck regenerates openapi.yaml and fails if it differs from the
// committed copy, catching handlers added without running make openapi.
func (c *Ci) OpenApiCheck(ctx context.Context, src *dagger.Directory) (string, error) {
	return dag.Container().
		From("alpine:latest").
		WithFile("/generated/openapi.yaml", c.openApiSpec(src)).
		WithFile("/committed/openapi.yaml", src.File("openapi.yaml")).
		WithExec([]string{"sh", "-c",
			`if ! diff -u /committed/openapi.yaml /generated/openapi.yaml; then
               echo "openapi.yaml is out of date — run 'make openapi' and commit the result"
               exit 1
             fi
             echo "openapi.yaml is up to date"`}).
		Stdout(ctx)
}

// PythonSdkLint regenerates the Python SDK from the current openapi spec and
// runs ruff check to verify the generated code is clean.
// Fails if the spec is stale (shares openApiSpec with OpenApiCheck).
func (c *Ci) PythonSdkLint(ctx context.Context, src *dagger.Directory) (string, error) {
	return dag.Container().
		From("ghcr.io/astral-sh/uv:python3.12-bookworm-slim").
		WithMountedCache("/root/.cache/uv", dag.CacheVolume("uv-cache")).
		WithDirectory("/work", src.Directory("python-sdk")).
		WithWorkdir("/work").
		WithExec([]string{"uv", "sync", "--group", "dev"}).
		WithFile("/work/openapi.yaml", c.openApiSpec(src)).
		WithExec([]string{"uv", "run", "openapi-python-client", "generate",
			"--path", "openapi.yaml",
			"--meta", "setup",
			"--overwrite",
			"--custom-template-path", "templates"}).
		WithExec([]string{"uvx", "ruff", "check", "."}).
		Stdout(ctx)
}

// QaraxImage builds and returns the qarax control-plane container image.
// Chain .Publish(ctx, address) to push to a registry.
func (c *Ci) QaraxImage(ctx context.Context, src *dagger.Directory) (*dagger.Container, error) {
	binDir, err := c.Build(ctx, src)
	if err != nil {
		return nil, fmt.Errorf("build: %w", err)
	}
	buildCtx := src.WithDirectory("target/x86_64-unknown-linux-musl/release", binDir)
	return buildCtx.DockerBuild(dagger.DirectoryDockerBuildOpts{
		Dockerfile: "e2e/Dockerfile.qarax",
	}), nil
}

// QaraxNodeImage builds and returns the qarax-node container image.
// Reads the hypervisor versions from versions/cloud-hypervisor-version and
// versions/firecracker-version.
// Chain .Publish(ctx, address) to push to a registry.
func (c *Ci) QaraxNodeImage(ctx context.Context, src *dagger.Directory) (*dagger.Container, error) {
	binDir, err := c.Build(ctx, src)
	if err != nil {
		return nil, fmt.Errorf("build: %w", err)
	}
	chVersion, err := src.File("versions/cloud-hypervisor-version").Contents(ctx)
	if err != nil {
		return nil, fmt.Errorf("read cloud-hypervisor version: %w", err)
	}
	firecrackerVersion, err := src.File("versions/firecracker-version").Contents(ctx)
	if err != nil {
		return nil, fmt.Errorf("read firecracker version: %w", err)
	}
	buildCtx := src.WithDirectory("target/x86_64-unknown-linux-musl/release", binDir)
	return buildCtx.DockerBuild(dagger.DirectoryDockerBuildOpts{
		Dockerfile: "e2e/Dockerfile.qarax-node",
		BuildArgs: []dagger.BuildArg{
			{Name: "CLOUD_HYPERVISOR_VERSION", Value: strings.TrimSpace(chVersion)},
			{Name: "FIRECRACKER_VERSION", Value: strings.TrimSpace(firecrackerVersion)},
		},
	}), nil
}

// All runs Fmt, Lint, Test, SqlxCheck, Audit, OpenApiCheck, and PythonSdkLint in parallel.
// Lint, Test, SqlxCheck, and OpenApiCheck all chain from prebuilt so the workspace
// is compiled exactly once before checks fan out.
// Use Build, QaraxImage, and QaraxNodeImage separately when you need binaries or images.
func (c *Ci) All(ctx context.Context, src *dagger.Directory) (string, error) {
	g, ctx := errgroup.WithContext(ctx)

	g.Go(func() error {
		if _, err := c.Fmt(ctx, src); err != nil {
			return fmt.Errorf("fmt: %w", err)
		}
		return nil
	})
	g.Go(func() error {
		if _, err := c.Lint(ctx, src); err != nil {
			return fmt.Errorf("lint: %w", err)
		}
		return nil
	})
	g.Go(func() error {
		if _, err := c.Test(ctx, src); err != nil {
			return fmt.Errorf("test: %w", err)
		}
		return nil
	})
	g.Go(func() error {
		if _, err := c.SqlxCheck(ctx, src); err != nil {
			return fmt.Errorf("sqlx-check: %w", err)
		}
		return nil
	})
	g.Go(func() error {
		if _, err := c.Audit(ctx, src); err != nil {
			return fmt.Errorf("audit: %w", err)
		}
		return nil
	})
	g.Go(func() error {
		if _, err := c.OpenApiCheck(ctx, src); err != nil {
			return fmt.Errorf("openapi-check: %w", err)
		}
		return nil
	})
	g.Go(func() error {
		if _, err := c.PythonSdkLint(ctx, src); err != nil {
			return fmt.Errorf("python-sdk-lint: %w", err)
		}
		return nil
	})

	if err := g.Wait(); err != nil {
		return "", err
	}
	return "All CI checks passed", nil
}
