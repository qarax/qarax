use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::oci_config::OciImageConfigDetails;
use tokio::task;

const DEFAULT_PATH: &[&str] = &[
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightCheckResult {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

impl PreflightCheckResult {
    pub fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: true,
            detail: detail.into(),
        }
    }

    pub fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: false,
            detail: detail.into(),
        }
    }
}

pub fn boot_mode_check(boot_mode: &str) -> PreflightCheckResult {
    if boot_mode == "kernel" {
        PreflightCheckResult::ok(
            "boot_mode",
            "OCI image boot is supported with kernel boot mode",
        )
    } else {
        PreflightCheckResult::fail(
            "boot_mode",
            "OCI image boot currently requires boot_mode=kernel",
        )
    }
}

pub fn architecture_check(requested: &str, actual: &str) -> PreflightCheckResult {
    if requested.is_empty() || requested == actual {
        PreflightCheckResult::ok(
            "architecture",
            format!("node architecture {} matches requested target", actual),
        )
    } else {
        PreflightCheckResult::fail(
            "architecture",
            format!(
                "requested architecture {} does not match node architecture {}",
                requested, actual
            ),
        )
    }
}

pub async fn guest_command_check(
    config: &OciImageConfigDetails,
    rootfs: &Path,
) -> PreflightCheckResult {
    let config = config.clone();
    let rootfs = rootfs.to_path_buf();
    match task::spawn_blocking(move || validate_guest_command(&config, &rootfs)).await {
        Ok(Ok(detail)) => PreflightCheckResult::ok("guest_command", detail),
        Ok(Err(detail)) => PreflightCheckResult::fail("guest_command", detail),
        Err(e) => PreflightCheckResult::fail(
            "guest_command",
            format!("guest command validation task failed: {}", e),
        ),
    }
}

pub fn validate_guest_command(
    config: &OciImageConfigDetails,
    rootfs: &Path,
) -> Result<String, String> {
    let mut argv = config.entrypoint.clone().unwrap_or_default();
    argv.extend(config.cmd.clone().unwrap_or_default());

    if argv.is_empty() {
        let shell = rootfs.join("bin/sh");
        return if is_executable(&shell) {
            Ok("image provides executable /bin/sh for Qarax fallback".to_string())
        } else {
            Err(
                "image config has no entrypoint/cmd and rootfs does not contain executable /bin/sh"
                    .to_string(),
            )
        };
    }

    let program = &argv[0];
    let Some(resolved) = resolve_program_path(program, config.env.as_deref(), rootfs) else {
        if program.contains('/') {
            return Err(format!(
                "configured executable '{}' was not found in the guest rootfs",
                program
            ));
        }

        let path = configured_or_default_path(config.env.as_deref()).join(":");
        return Err(format!(
            "command '{}' was not found in PATH ({}) inside the guest rootfs",
            program, path
        ));
    };

    Ok(format!("guest command resolves to {}", resolved))
}

pub fn supported_layer_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "application/vnd.oci.image.layer.v1.tar+gzip"
            | "application/vnd.docker.image.rootfs.diff.tar.gzip"
            | "application/vnd.oci.image.layer.v1.tar"
    )
}

fn resolve_program_path(program: &str, env: Option<&[String]>, rootfs: &Path) -> Option<String> {
    if program.starts_with('/') {
        let candidate = rootfs_candidate(rootfs, program.trim_start_matches('/'))?;
        return is_executable(&candidate).then(|| program.to_string());
    }

    if program.contains('/') {
        let candidate = rootfs_candidate(rootfs, program)?;
        return is_executable(&candidate).then(|| format!("/{}", program.trim_start_matches('/')));
    }

    for entry in configured_or_default_path(env) {
        let relative = entry.trim_start_matches('/');
        let candidate = if relative.is_empty() {
            rootfs.join(program)
        } else {
            rootfs.join(relative).join(program)
        };
        if is_executable(&candidate) {
            return Some(if relative.is_empty() {
                format!("/{}", program)
            } else {
                format!("/{}/{}", relative, program)
            });
        }
    }

    None
}

fn configured_or_default_path(env: Option<&[String]>) -> Vec<String> {
    env.and_then(|vars| {
        vars.iter()
            .find_map(|var| var.strip_prefix("PATH=").map(str::to_string))
    })
    .map(|path| path.split(':').map(str::to_string).collect())
    .unwrap_or_else(|| DEFAULT_PATH.iter().map(|entry| entry.to_string()).collect())
}

fn rootfs_candidate(rootfs: &Path, relative: &str) -> Option<std::path::PathBuf> {
    (!relative.is_empty()).then(|| rootfs.join(relative))
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && (metadata.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{supported_layer_media_type, validate_guest_command};
    use crate::oci_config::OciImageConfigDetails;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn make_exec(root: &TempDir, relative: &str) {
        let path = root.path().join(relative.trim_start_matches('/'));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn supports_expected_layer_types() {
        assert!(supported_layer_media_type(
            "application/vnd.oci.image.layer.v1.tar+gzip"
        ));
        assert!(supported_layer_media_type(
            "application/vnd.docker.image.rootfs.diff.tar.gzip"
        ));
        assert!(supported_layer_media_type(
            "application/vnd.oci.image.layer.v1.tar"
        ));
        assert!(!supported_layer_media_type(
            "application/vnd.oci.image.layer.v1.tar+zstd"
        ));
    }

    #[test]
    fn empty_command_requires_bin_sh() {
        let root = TempDir::new().unwrap();
        make_exec(&root, "/bin/sh");
        let config = OciImageConfigDetails::default();

        let detail = validate_guest_command(&config, root.path()).unwrap();
        assert!(detail.contains("/bin/sh"));
    }

    #[test]
    fn empty_command_without_shell_fails() {
        let root = TempDir::new().unwrap();
        let config = OciImageConfigDetails::default();

        let err = validate_guest_command(&config, root.path()).unwrap_err();
        assert!(err.contains("/bin/sh"));
    }

    #[test]
    fn absolute_entrypoint_must_exist() {
        let root = TempDir::new().unwrap();
        make_exec(&root, "/usr/bin/app");
        let config = OciImageConfigDetails {
            entrypoint: Some(vec!["/usr/bin/app".to_string()]),
            ..Default::default()
        };

        let detail = validate_guest_command(&config, root.path()).unwrap();
        assert!(detail.contains("/usr/bin/app"));
    }

    #[test]
    fn bare_command_uses_image_path() {
        let root = TempDir::new().unwrap();
        make_exec(&root, "/usr/local/bin/server");
        let config = OciImageConfigDetails {
            entrypoint: Some(vec!["server".to_string()]),
            env: Some(vec!["PATH=/usr/local/bin:/usr/bin".to_string()]),
            ..Default::default()
        };

        let detail = validate_guest_command(&config, root.path()).unwrap();
        assert!(detail.contains("/usr/local/bin/server"));
    }

    #[test]
    fn bare_command_uses_default_path_when_unset() {
        let root = TempDir::new().unwrap();
        make_exec(&root, "/bin/busybox");
        let config = OciImageConfigDetails {
            entrypoint: Some(vec!["busybox".to_string()]),
            ..Default::default()
        };

        let detail = validate_guest_command(&config, root.path()).unwrap();
        assert!(detail.contains("/bin/busybox"));
    }
}
