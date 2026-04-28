use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    api::{
        self,
        models::{NewSecurityGroup, NewSecurityGroupRule},
    },
    client::Client,
};

use super::{OutputFormat, print_output, resolve_security_group_id};

#[derive(Args)]
pub struct SecurityGroupArgs {
    #[command(subcommand)]
    command: SecurityGroupCommand,
}

#[derive(Subcommand)]
enum SecurityGroupCommand {
    /// List security groups
    List,
    /// Get a security group
    Get {
        /// Security group name or ID
        security_group: String,
    },
    /// Create a security group
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a security group
    Delete {
        /// Security group name or ID
        security_group: String,
    },
    /// List rules in a security group
    ListRules {
        /// Security group name or ID
        security_group: String,
    },
    /// Add a rule to a security group
    AddRule {
        /// Security group name or ID
        #[arg(long)]
        security_group: String,
        #[arg(long)]
        direction: String,
        #[arg(long)]
        protocol: String,
        #[arg(long)]
        cidr: Option<String>,
        #[arg(long)]
        port_start: Option<i32>,
        #[arg(long)]
        port_end: Option<i32>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a rule from a security group
    DeleteRule {
        /// Security group name or ID
        #[arg(long)]
        security_group: String,
        /// Rule UUID
        #[arg(long)]
        rule_id: String,
    },
}

#[derive(Tabled)]
struct SecurityGroupRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Description")]
    description: String,
}

#[derive(Tabled)]
struct SecurityGroupRuleRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Direction")]
    direction: String,
    #[tabled(rename = "Protocol")]
    protocol: String,
    #[tabled(rename = "CIDR")]
    cidr: String,
    #[tabled(rename = "Ports")]
    ports: String,
    #[tabled(rename = "Description")]
    description: String,
}

pub async fn run(
    args: SecurityGroupArgs,
    client: &Client,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match args.command {
        SecurityGroupCommand::List => {
            let groups = api::security_groups::list(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&groups, output)?;
            } else {
                let rows: Vec<_> = groups
                    .iter()
                    .map(|group| SecurityGroupRow {
                        id: group.id.to_string(),
                        name: group.name.clone(),
                        description: group.description.clone().unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
        SecurityGroupCommand::Get { security_group } => {
            let id = resolve_security_group_id(client, &security_group).await?;
            let group = api::security_groups::get(client, id).await?;
            let rules = api::security_groups::list_rules(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(
                    &serde_json::json!({ "group": group, "rules": rules }),
                    output,
                )?;
            } else {
                println!("ID:          {}", group.id);
                println!("Name:        {}", group.name);
                println!(
                    "Description: {}",
                    group.description.unwrap_or_else(|| "-".to_string())
                );
                println!("Rules:       {}", rules.len());
            }
        }
        SecurityGroupCommand::Create { name, description } => {
            let id = api::security_groups::create(client, &NewSecurityGroup { name, description })
                .await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "security_group_id": id }), output)?;
            } else {
                println!("Created security group: {id}");
            }
        }
        SecurityGroupCommand::Delete { security_group } => {
            let id = resolve_security_group_id(client, &security_group).await?;
            api::security_groups::delete(client, id).await?;
            println!("Deleted security group: {security_group}");
        }
        SecurityGroupCommand::ListRules { security_group } => {
            let id = resolve_security_group_id(client, &security_group).await?;
            let rules = api::security_groups::list_rules(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&rules, output)?;
            } else {
                let rows: Vec<_> = rules
                    .iter()
                    .map(|rule| SecurityGroupRuleRow {
                        id: rule.id.to_string(),
                        direction: rule.direction.clone(),
                        protocol: rule.protocol.clone(),
                        cidr: rule.cidr.clone().unwrap_or_else(|| "0.0.0.0/0".to_string()),
                        ports: match (rule.port_start, rule.port_end) {
                            (Some(start), Some(end)) if start == end => start.to_string(),
                            (Some(start), Some(end)) => format!("{start}-{end}"),
                            _ => "-".to_string(),
                        },
                        description: rule.description.clone().unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
        SecurityGroupCommand::AddRule {
            security_group,
            direction,
            protocol,
            cidr,
            port_start,
            port_end,
            description,
        } => {
            let id = resolve_security_group_id(client, &security_group).await?;
            let rule_id = api::security_groups::create_rule(
                client,
                id,
                &NewSecurityGroupRule {
                    direction,
                    protocol,
                    cidr,
                    port_start,
                    port_end,
                    description,
                },
            )
            .await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "rule_id": rule_id }), output)?;
            } else {
                println!("Created rule: {rule_id}");
            }
        }
        SecurityGroupCommand::DeleteRule {
            security_group,
            rule_id,
        } => {
            let security_group_id = resolve_security_group_id(client, &security_group).await?;
            let rule_id = uuid::Uuid::parse_str(&rule_id)?;
            api::security_groups::delete_rule(client, security_group_id, rule_id).await?;
            println!("Deleted rule {rule_id}");
        }
    }

    Ok(())
}
