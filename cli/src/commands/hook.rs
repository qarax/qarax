use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    api::{
        self,
        models::{NewLifecycleHook, UpdateLifecycleHook},
    },
    client::Client,
};

use super::{OutputFormat, print_output, resolve_hook_id};

#[derive(Args)]
pub struct HookArgs {
    #[command(subcommand)]
    command: HookCommand,
}

#[derive(Subcommand)]
enum HookCommand {
    /// List all lifecycle hooks
    List,
    /// Get details of a lifecycle hook
    Get {
        /// Hook name or ID
        hook: String,
    },
    /// Create a new lifecycle hook
    Create {
        /// Hook name
        #[arg(long)]
        name: String,
        /// Webhook URL to POST events to
        #[arg(long)]
        url: String,
        /// Hook scope: global, vm, or tag
        #[arg(long, default_value = "global")]
        scope: String,
        /// Scope value (VM ID for vm scope, tag name for tag scope)
        #[arg(long)]
        scope_value: Option<String>,
        /// Comma-separated list of events to trigger on (empty = all)
        #[arg(long)]
        events: Option<String>,
        /// HMAC secret for payload signing
        #[arg(long)]
        secret: Option<String>,
    },
    /// Update a lifecycle hook
    Update {
        /// Hook name or ID
        hook: String,
        /// New webhook URL
        #[arg(long)]
        url: Option<String>,
        /// Enable or disable the hook
        #[arg(long)]
        active: Option<bool>,
        /// New scope: global, vm, or tag
        #[arg(long)]
        scope: Option<String>,
        /// New scope value
        #[arg(long)]
        scope_value: Option<String>,
        /// Clear the scope value
        #[arg(long, conflicts_with = "scope_value")]
        clear_scope_value: bool,
        /// New comma-separated events list
        #[arg(long)]
        events: Option<String>,
        /// New HMAC secret
        #[arg(long)]
        secret: Option<String>,
        /// Clear the HMAC secret
        #[arg(long, conflicts_with = "secret")]
        clear_secret: bool,
    },
    /// Delete a lifecycle hook
    Delete {
        /// Hook name or ID
        hook: String,
    },
    /// List executions of a lifecycle hook
    Executions {
        /// Hook name or ID
        hook: String,
    },
}

#[derive(Tabled)]
struct HookRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "URL")]
    url: String,
    #[tabled(rename = "Scope")]
    scope: String,
    #[tabled(rename = "Events")]
    events: String,
    #[tabled(rename = "Active")]
    active: String,
}

#[derive(Tabled)]
struct ExecutionRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "VM")]
    vm_id: String,
    #[tabled(rename = "Prev Status")]
    previous_status: String,
    #[tabled(rename = "New Status")]
    new_status: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Attempts")]
    attempts: String,
    #[tabled(rename = "Created")]
    created_at: String,
}

pub async fn run(args: HookArgs, client: &Client, output: OutputFormat) -> anyhow::Result<()> {
    match args.command {
        HookCommand::List => {
            let hooks = api::hooks::list(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&hooks, output)?;
            } else {
                let rows: Vec<HookRow> = hooks
                    .iter()
                    .map(|h| {
                        let scope_display = match h.scope_value.as_deref() {
                            Some(v) => format!("{}:{}", h.scope, v),
                            None => h.scope.clone(),
                        };
                        let events_display = if h.events.is_empty() {
                            "*".to_string()
                        } else {
                            h.events.join(",")
                        };
                        HookRow {
                            id: h.id.to_string(),
                            name: h.name.clone(),
                            url: h.url.clone(),
                            scope: scope_display,
                            events: events_display,
                            active: h.active.to_string(),
                        }
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }

        HookCommand::Get { hook } => {
            let id = resolve_hook_id(client, &hook).await?;
            let h = api::hooks::get(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&h, output)?;
            } else {
                println!("ID:          {}", h.id);
                println!("Name:        {}", h.name);
                println!("URL:         {}", h.url);
                println!("Scope:       {}", h.scope);
                if let Some(ref sv) = h.scope_value {
                    println!("Scope Value: {sv}");
                }
                let events_display = if h.events.is_empty() {
                    "* (all)".to_string()
                } else {
                    h.events.join(", ")
                };
                println!("Events:      {events_display}");
                println!("Active:      {}", h.active);
                println!("Created:     {}", h.created_at);
                println!("Updated:     {}", h.updated_at);
            }
        }

        HookCommand::Create {
            name,
            url,
            scope,
            scope_value,
            events,
            secret,
        } => {
            let events_vec = events
                .map(|e| e.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();
            let new_hook = NewLifecycleHook {
                name,
                url,
                secret,
                scope,
                scope_value,
                events: events_vec,
            };
            let id = api::hooks::create(client, &new_hook).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "hook_id": id }), output)?;
            } else {
                println!("Created hook: {id}");
            }
        }

        HookCommand::Update {
            hook,
            url,
            active,
            scope,
            scope_value,
            clear_scope_value,
            events,
            secret,
            clear_secret,
        } => {
            let id = resolve_hook_id(client, &hook).await?;
            let events_vec = events.map(|e| e.split(',').map(|s| s.trim().to_string()).collect());
            let req = UpdateLifecycleHook {
                url,
                secret: if clear_secret {
                    Some(None)
                } else {
                    secret.map(Some)
                },
                scope,
                scope_value: if clear_scope_value {
                    Some(None)
                } else {
                    scope_value.map(Some)
                },
                events: events_vec,
                active,
            };
            let updated = api::hooks::update(client, id, &req).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&updated, output)?;
            } else {
                println!("Updated hook: {}", updated.name);
            }
        }

        HookCommand::Delete { hook } => {
            let id = resolve_hook_id(client, &hook).await?;
            api::hooks::delete(client, id).await?;
            println!("Deleted hook: {id}");
        }

        HookCommand::Executions { hook } => {
            let id = resolve_hook_id(client, &hook).await?;
            let executions = api::hooks::list_executions(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&executions, output)?;
            } else {
                let rows: Vec<ExecutionRow> = executions
                    .iter()
                    .map(|e| ExecutionRow {
                        id: e.id.to_string(),
                        vm_id: e.vm_id.to_string(),
                        previous_status: e.previous_status.clone(),
                        new_status: e.new_status.clone(),
                        status: e.status.clone(),
                        attempts: format!("{}/{}", e.attempt_count, e.max_attempts),
                        created_at: e.created_at.clone(),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
    }

    Ok(())
}
