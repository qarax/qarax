use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};
use uuid::Uuid;

use crate::{
    api::{
        self,
        models::{ConfigureSandboxPoolRequest, ExecSandboxRequest, NewSandbox},
    },
    client::Client,
};

use super::{OutputFormat, print_output, resolve_network_id, resolve_vm_template_id};

#[derive(Args)]
pub struct SandboxArgs {
    #[command(subcommand)]
    command: SandboxCommand,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum SandboxCommand {
    /// List all sandboxes
    List,
    /// Manage prewarmed sandbox pools
    Pool {
        #[command(subcommand)]
        command: SandboxPoolCommand,
    },
    /// Get details of a sandbox
    Get {
        /// Sandbox name or ID
        sandbox: String,
    },
    /// Create and start a new sandbox from a VM template
    Create {
        /// VM template name or ID to base the sandbox on
        #[arg(long)]
        template: String,
        /// Sandbox name (auto-generated if omitted)
        #[arg(long)]
        name: Option<String>,
        /// Idle timeout in seconds before the sandbox is auto-deleted (default: 300)
        #[arg(long)]
        idle_timeout: Option<i32>,
        /// Network name or ID to attach the sandbox to
        #[arg(long)]
        network: Option<String>,
        /// Block until the sandbox reaches 'ready' state
        #[arg(short, long)]
        wait: bool,
    },
    /// Delete a sandbox and its underlying VM
    Delete {
        /// Sandbox name or ID
        sandbox: String,
    },
    /// Execute a command inside a running sandbox
    Exec {
        /// Sandbox name or ID
        sandbox: String,
        /// Kill the guest command if it runs longer than this many seconds
        #[arg(long)]
        timeout: Option<u64>,
        /// Command and arguments to execute inside the sandbox
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand)]
enum SandboxPoolCommand {
    /// List configured sandbox pools
    List,
    /// Get the sandbox pool for a VM template
    Get {
        /// VM template name or ID
        #[arg(long)]
        template: String,
    },
    /// Configure the sandbox pool for a VM template
    Set {
        /// VM template name or ID
        #[arg(long)]
        template: String,
        /// Keep at least this many prewarmed sandboxes ready
        #[arg(long)]
        min_ready: i32,
    },
    /// Delete the sandbox pool for a VM template
    Delete {
        /// VM template name or ID
        #[arg(long)]
        template: String,
    },
}

#[derive(Tabled)]
struct SandboxRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "VM Status")]
    vm_status: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "Idle Timeout")]
    idle_timeout: String,
}

#[derive(Tabled)]
struct SandboxPoolRow {
    #[tabled(rename = "Template")]
    template: String,
    #[tabled(rename = "Min Ready")]
    min_ready: i32,
    #[tabled(rename = "Ready")]
    ready: i64,
    #[tabled(rename = "Provisioning")]
    provisioning: i64,
    #[tabled(rename = "Error")]
    error: i64,
}

pub async fn run(args: SandboxArgs, client: &Client, output: OutputFormat) -> anyhow::Result<()> {
    match args.command {
        SandboxCommand::List => {
            let sandboxes = api::sandboxes::list(client).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&sandboxes, output)?;
            } else {
                let rows: Vec<SandboxRow> = sandboxes
                    .iter()
                    .map(|s| SandboxRow {
                        id: s.id.to_string()[..8].to_string(),
                        name: s.name.clone(),
                        status: s.status.clone(),
                        vm_status: s.vm_status.clone().unwrap_or_else(|| "-".to_string()),
                        ip: s.ip_address.clone().unwrap_or_else(|| "-".to_string()),
                        idle_timeout: format!("{}s", s.idle_timeout_secs),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
        SandboxCommand::Pool { command } => match command {
            SandboxPoolCommand::List => {
                let pools = api::sandbox_pools::list(client).await?;
                if !matches!(output, OutputFormat::Table) {
                    print_output(&pools, output)?;
                } else {
                    let rows: Vec<SandboxPoolRow> = pools
                        .iter()
                        .map(|pool| SandboxPoolRow {
                            template: pool.vm_template_name.clone(),
                            min_ready: pool.min_ready,
                            ready: pool.current_ready,
                            provisioning: pool.current_provisioning,
                            error: pool.current_error,
                        })
                        .collect();
                    println!("{}", Table::new(rows).with(Style::psql()));
                }
            }
            SandboxPoolCommand::Get { template } => {
                let vm_template_id = resolve_vm_template_id(client, &template).await?;
                let pool = api::sandbox_pools::get(client, vm_template_id).await?;
                if !matches!(output, OutputFormat::Table) {
                    print_output(&pool, output)?;
                } else {
                    println!("Template:      {}", pool.vm_template_name);
                    println!("Template ID:   {}", pool.vm_template_id);
                    println!("Min Ready:     {}", pool.min_ready);
                    println!("Ready:         {}", pool.current_ready);
                    println!("Provisioning:  {}", pool.current_provisioning);
                    println!("Error:         {}", pool.current_error);
                    println!("Created:       {}", pool.created_at);
                    println!("Updated:       {}", pool.updated_at);
                }
            }
            SandboxPoolCommand::Set {
                template,
                min_ready,
            } => {
                let vm_template_id = resolve_vm_template_id(client, &template).await?;
                let pool = api::sandbox_pools::put(
                    client,
                    vm_template_id,
                    &ConfigureSandboxPoolRequest { min_ready },
                )
                .await?;
                if !matches!(output, OutputFormat::Table) {
                    print_output(&pool, output)?;
                } else {
                    println!(
                        "Configured sandbox pool for {}: min_ready={}",
                        pool.vm_template_name, pool.min_ready
                    );
                }
            }
            SandboxPoolCommand::Delete { template } => {
                let vm_template_id = resolve_vm_template_id(client, &template).await?;
                api::sandbox_pools::delete(client, vm_template_id).await?;
                println!("Deleted sandbox pool for template: {template}");
            }
        },
        SandboxCommand::Get { sandbox } => {
            let id = resolve_sandbox_id(client, &sandbox).await?;
            let s = api::sandboxes::get(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&s, output)?;
            } else {
                println!("ID:           {}", s.id);
                println!("Name:         {}", s.name);
                println!("Status:       {}", s.status);
                if let Some(vm_status) = &s.vm_status {
                    println!("VM Status:    {vm_status}");
                }
                println!("VM ID:        {}", s.vm_id);
                if let Some(ip) = &s.ip_address {
                    println!("IP:           {ip}");
                }
                println!("Idle Timeout: {}s", s.idle_timeout_secs);
                println!("Created:      {}", s.created_at);
                println!("Last Active:  {}", s.last_activity_at);
                if let Some(err) = &s.error_message {
                    println!("Error:        {err}");
                }
            }
        }
        SandboxCommand::Create {
            template,
            name,
            idle_timeout,
            network,
            wait,
        } => {
            let vm_template_id = resolve_vm_template_id(client, &template).await?;
            let network_id = match network {
                Some(ref n) => Some(resolve_network_id(client, n).await?),
                None => None,
            };
            let name =
                name.unwrap_or_else(|| format!("sandbox-{}", &Uuid::new_v4().to_string()[..8]));
            let req = NewSandbox {
                name,
                vm_template_id,
                idle_timeout_secs: idle_timeout,
                instance_type_id: None,
                network_id,
            };
            let resp = api::sandboxes::create(client, &req).await?;
            if wait {
                crate::wait::wait_for_sandbox(client, resp.id).await?;
            }
            if !matches!(output, OutputFormat::Table) {
                print_output(&resp, output)?;
            } else {
                println!("Sandbox created: {}", resp.id);
                println!("VM ID:          {}", resp.vm_id);
                println!("Job ID:         {}", resp.job_id);
                if !wait {
                    println!("Poll `qarax sandbox get {}` to check status.", resp.id);
                }
            }
        }
        SandboxCommand::Delete { sandbox } => {
            let id = resolve_sandbox_id(client, &sandbox).await?;
            api::sandboxes::delete(client, id).await?;
            println!("Deleted sandbox: {id}");
        }
        SandboxCommand::Exec {
            sandbox,
            timeout,
            command,
        } => {
            let id = resolve_sandbox_id(client, &sandbox).await?;
            let response = api::sandboxes::exec(
                client,
                id,
                &ExecSandboxRequest {
                    command,
                    timeout_secs: timeout,
                },
            )
            .await?;

            if !matches!(output, OutputFormat::Table) {
                print_output(&response, output)?;
            } else {
                if !response.stdout.is_empty() {
                    print!("{}", response.stdout);
                }
                if !response.stderr.is_empty() {
                    eprint!("{}", response.stderr);
                }
                if response.timed_out {
                    anyhow::bail!("sandbox command timed out");
                }
                if response.exit_code != 0 {
                    anyhow::bail!("sandbox command exited with status {}", response.exit_code);
                }
            }
        }
    }

    Ok(())
}

/// Resolve a sandbox name or UUID string to a UUID.
pub async fn resolve_sandbox_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let sandboxes = api::sandboxes::list(client).await?;
    sandboxes
        .into_iter()
        .find(|s| s.name == name_or_id)
        .map(|s| s.id)
        .ok_or_else(|| anyhow::anyhow!("no sandbox named {:?}", name_or_id))
}
