use anyhow::{anyhow, Context, Result};
use std::{collections::BTreeMap, fmt};
use tokio::process::Command;

const CMD: &str = "/usr/bin/ansible-playbook";
pub const INSTALL_HOST_PLAYBOOK: &str = "playbooks/roles/setup_host/playbook.yml";

#[derive(Debug)]
pub struct AnsibleCommand<'a> {
    playbook: &'a str,
    user: &'a str,
    host: &'a str,
    extra_params: BTreeMap<String, String>,
}

impl<'a> AnsibleCommand<'a> {
    pub fn new(
        playbook: &'a str,
        user: &'a str,
        host: &'a str,
        extra_params: BTreeMap<String, String>,
    ) -> Self {
        AnsibleCommand {
            playbook,
            user,
            host,
            extra_params,
        }
    }

    fn build_args(&self) -> Vec<String> {
        let mut args = vec![
            self.playbook.to_string(),
            format!("-i {},", self.host),
            format!("-u {}", self.user),
        ];

        for (k, v) in self.extra_params.iter() {
            args.push(format!("-e {}={}", k, v));
        }

        args
    }

    pub async fn run_playbook(&self) -> Result<()> {
        // TODO: handle errors and write output properly
        let mut process = Command::new(CMD)
            .args(self.build_args())
            .spawn()
            .context("Ansible failed")?;

        let wait_result = process.wait().await;
        match wait_result {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else {
                    Err(anyhow!("playbook failed, '{}'", status))
                }
            }
            Err(e) => Err(e).context("Error waiting for Ansible process"),
        }
    }
}

impl<'a> fmt::Display for AnsibleCommand<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut output = format!("{} -i {}, -u {}", self.playbook, self.host, self.user);

        for (k, v) in self.extra_params.iter() {
            output.push_str(format!(" -e {}={}", k, v).as_str());
        }

        write!(f, "{}", output)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_generate_command() {
        let mut extra_params = BTreeMap::new();
        extra_params.insert(String::from("ansible_password"), String::from("fedora"));

        // TODO: update versions
        extra_params.insert(String::from("fcversion"), String::from("0.21.1"));

        let ac = AnsibleCommand::new(CMD, "root", "192.168.122.45", extra_params);
        const OUTPUT: &str = "/usr/bin/ansible-playbook -i 192.168.122.45, -u root -e ansible_password=fedora -e fcversion=0.21.1";

        assert_eq!(ac.to_string(), OUTPUT);
    }
}
