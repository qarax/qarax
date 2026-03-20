---
description: How to add a new feature or command to the CLI
---

# Workflow: Implementing a New CLI Feature

Follow these steps when adding new commands, features, or arguments to the Qarax command-line application (`cli` crate).

## 1. Update Data Models
If the new feature requires a payload or API response not currently defined in the CLI, add it to `cli/src/api/models.rs`.
- Ensure structs derive `Serialize` and/or `Deserialize`.
- Do not import models from the server crates (`qarax`); redefine them to keep the CLI decoupled.

## 2. Add API Wrapper
Create or update a method in the corresponding resource file under `cli/src/api/` (e.g., `vms.rs`, `storage.rs`).
- The method signature should take the `cli::client::Client` and any required parameters.
- Use the client's typed helper methods: `client.get(...)`, `client.post(...)`, `client.delete(...)`, etc.
- If you create a new file, re-export it in `cli/src/api/mod.rs`.

## 3. Define CLI Interface
In `cli/src/commands/<resource>.rs`, create or update the `Args` and `Command` definitions.
- Use `clap` macros to define arguments and subcommands: `#[derive(Parser)]`, `#[derive(Subcommand)]`, `#[derive(Args)]`.
- Document arguments clearly via triple-slash comments (`///`), as these become the CLI help text.

## 4. Implement Command Logic
Update the `run()` function in `cli/src/commands/<resource>.rs` to execute the command.
- **Resolution**: Use the resolution helpers in `cli/src/commands/mod.rs` (e.g., `resolve_vm_id`) if the resource supports being looked up by user-defined names. Top-level entities (VMs, Hosts, Networks, Storage) and some child resources (Snapshots) should accept "name or ID", whereas system-generated entities without user-configurable names (like Jobs) should strictly accept UUIDs.
- **Execution**: Await your API wrapper function.
- **Jobs**: If the API returns a `job_id`, use a polling loop (like `poll_job`) to provide visual progress to the user rather than returning instantly.
- **Output**:
  - Always respect the global `OutputFormat` config (`json`, `yaml`, `table`).
  - Use `print_output(&response, output)` to automatically handle `json` and `yaml`.
  - For tables, define a custom `<Resource>Row` struct deriving `Tabled` and format fields nicely.
  - Print the table with `println!("{}", Table::new(rows).with(Style::psql()))`.

## 5. Register the Command
If you created a new top-level subcommand:
- Register the `Args` variant in the `Commands` enum in `cli/src/main.rs`.
- Add the corresponding match arm in the `main()` function to route the call to your `run()` function.
