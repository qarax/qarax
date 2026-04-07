use anyhow::Result;
use clap::{Parser, Subcommand};
use commands::OutputFormat;

mod api;
mod client;
mod commands;
mod config;
mod console;
mod wait;

const DEFAULT_SERVER: &str = "http://127.0.0.1:8000";

pub fn resolve_server(flag: Option<String>, cfg: &config::Config) -> String {
    flag.or_else(|| cfg.server.clone())
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::Config;

    fn cfg(server: Option<&str>) -> Config {
        Config {
            server: server.map(str::to_string),
        }
    }

    #[test]
    fn flag_beats_config_and_default() {
        let server = resolve_server(
            Some("http://flag:8000".to_string()),
            &cfg(Some("http://config:8000")),
        );
        assert_eq!(server, "http://flag:8000");
    }

    #[test]
    fn config_beats_default() {
        let server = resolve_server(None, &cfg(Some("http://config:8000")));
        assert_eq!(server, "http://config:8000");
    }

    #[test]
    fn falls_back_to_default() {
        let server = resolve_server(None, &cfg(None));
        assert_eq!(server, DEFAULT_SERVER);
    }
}

#[derive(Parser)]
#[command(name = "qarax", about = "CLI for the qarax VM management API", version)]
pub struct Cli {
    /// Server base URL (overrides config file and QARAX_SERVER env var)
    #[arg(long, env = "QARAX_SERVER", global = true)]
    pub server: Option<String>,

    /// Output format (table, json, yaml)
    #[arg(
        short = 'o',
        long = "output",
        global = true,
        default_value = "table",
        value_name = "FORMAT"
    )]
    pub output: OutputFormat,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    /// Virtual machine operations
    Vm(commands::vm::VmArgs),
    /// Hypervisor host operations
    Host(commands::host::HostArgs),
    /// Storage pool operations
    StoragePool(commands::storage::StoragePoolArgs),
    /// Storage object operations
    StorageObject(commands::storage::StorageObjectArgs),
    /// File transfer operations
    Transfer(commands::transfer::TransferArgs),
    /// Boot source operations
    BootSource(commands::boot_source::BootSourceArgs),
    /// Lifecycle hook operations
    Hook(commands::hook::HookArgs),
    /// Instance type operations
    InstanceType(commands::instance_type::InstanceTypeArgs),
    /// VM template operations
    VmTemplate(commands::vm_template::VmTemplateArgs),
    /// Network operations
    Network(commands::network::NetworkArgs),
    /// Async job operations
    Job(commands::job::JobArgs),
    /// Sandbox operations (ephemeral microVM environments for AI agents)
    Sandbox(commands::sandbox::SandboxArgs),
    /// Configure the CLI (server URL, etc.)
    Configure(commands::configure::ConfigureArgs),
    /// Audit log operations
    AuditLog(commands::audit_log::AuditLogArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Commands::Configure(args) = cli.command {
        return commands::configure::run(args).await;
    }

    let cfg = config::load();
    let server = resolve_server(cli.server, &cfg);

    let client = client::Client::new(&server);

    match cli.command {
        Commands::Vm(args) => commands::vm::run(args, &client, cli.output).await,
        Commands::Host(args) => commands::host::run(args, &client, cli.output).await,
        Commands::InstanceType(args) => {
            commands::instance_type::run(args, &client, cli.output).await
        }
        Commands::StoragePool(args) => commands::storage::run_pool(args, &client, cli.output).await,
        Commands::StorageObject(args) => {
            commands::storage::run_object(args, &client, cli.output).await
        }
        Commands::Network(args) => commands::network::run(args, &client, cli.output).await,
        Commands::Transfer(args) => commands::transfer::run(args, &client, cli.output).await,
        Commands::BootSource(args) => commands::boot_source::run(args, &client, cli.output).await,
        Commands::VmTemplate(args) => commands::vm_template::run(args, &client, cli.output).await,
        Commands::Hook(args) => commands::hook::run(args, &client, cli.output).await,
        Commands::Job(args) => commands::job::run(args, &client, cli.output).await,
        Commands::Sandbox(args) => commands::sandbox::run(args, &client, cli.output).await,
        Commands::AuditLog(args) => commands::audit_log::run(args, &client, cli.output).await,
        Commands::Configure(_) => unreachable!(),
    }
}
