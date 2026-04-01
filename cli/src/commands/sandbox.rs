use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};
use uuid::Uuid;

use crate::{
    api::{
        self,
        models::{ExecSandboxRequest, NewSandbox},
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
