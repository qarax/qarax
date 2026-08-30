use anyhow::Result;
use clap::Args;
use std::io::{self, Write};

use crate::config;

#[derive(Args)]
pub struct ConfigureArgs {
    /// Server URL to save (skips interactive prompt)
    #[arg(long)]
    pub server: Option<String>,

    /// API token to save (skips interactive prompt)
    #[arg(long)]
    pub token: Option<String>,
}

pub async fn run(args: ConfigureArgs) -> Result<()> {
    let mut cfg = config::load();

    let server = match args.server {
        Some(s) => s,
        None => {
            let current = cfg.server.as_deref().unwrap_or(crate::DEFAULT_SERVER);
            print!("Server URL [{}]: ", current);
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();
            if input.is_empty() {
                current.to_string()
            } else {
                input.to_string()
            }
        }
    };

    let token = match args.token {
        Some(t) => Some(t),
        None => {
            let prompt = match &cfg.token {
                Some(_) => "API token [unchanged, leave blank to keep]: ",
                None => "API token [none, leave blank to skip]: ",
            };
            print!("{prompt}");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();
            if input.is_empty() {
                cfg.token.clone()
            } else {
                Some(input.to_string())
            }
        }
    };

    cfg.server = Some(server.clone());
    cfg.token = token;
    config::save(&cfg)?;

    println!("Saved to {}", config::path_display());
    println!("  server = {server}");
    println!(
        "  token = {}",
        if cfg.token.is_some() {
            "<set>"
        } else {
            "<none>"
        }
    );

    Ok(())
}
