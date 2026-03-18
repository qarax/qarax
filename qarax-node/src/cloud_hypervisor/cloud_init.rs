//! Cloud-init NoCloud seed image generation.
//!
//! Produces a small VFAT disk image (labeled "CIDATA") containing the three
//! standard NoCloud files: `meta-data`, `user-data`, and optionally
//! `network-config`. Cloud Hypervisor exposes this as a read-only virtio-blk
//! device; cloud-init's NoCloud datasource discovers it by the volume label.

use std::io::{Cursor, Write};

/// 512 KiB — plenty for any reasonable cloud-init payload.
const SEED_IMAGE_SIZE: usize = 512 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CloudInitError {
    #[error("IO error generating cloud-init seed: {0}")]
    Io(#[from] std::io::Error),
    #[error("FAT filesystem error: {0}")]
    Fat(String),
}

/// Build a NoCloud seed image in memory and return the raw bytes.
///
/// `user_data` and `meta_data` are mandatory; `network_config` is optional.
/// The caller is responsible for writing the bytes to disk (use `tokio::fs::write`
/// to avoid blocking the async runtime).
pub fn build_seed_image(
    user_data: &str,
    meta_data: &str,
    network_config: Option<&str>,
) -> Result<Vec<u8>, CloudInitError> {
    let mut buf = vec![0u8; SEED_IMAGE_SIZE];

    // Format the buffer as FAT12 with the "CIDATA" volume label.
    // Cloud-init checks for "cidata" or "CIDATA" (case-insensitive on Linux).
    fatfs::format_volume(
        Cursor::new(buf.as_mut_slice()),
        fatfs::FormatVolumeOptions::new().volume_label(*b"CIDATA     "),
    )
    .map_err(|e| CloudInitError::Fat(e.to_string()))?;

    // Open the freshly formatted volume and write the cloud-init files.
    {
        let fs = fatfs::FileSystem::new(Cursor::new(buf.as_mut_slice()), fatfs::FsOptions::new())
            .map_err(|e| CloudInitError::Fat(e.to_string()))?;

        let root = fs.root_dir();

        root.create_file("meta-data")
            .map_err(|e| CloudInitError::Fat(e.to_string()))?
            .write_all(meta_data.as_bytes())?;

        root.create_file("user-data")
            .map_err(|e| CloudInitError::Fat(e.to_string()))?
            .write_all(user_data.as_bytes())?;

        if let Some(nc) = network_config {
            root.create_file("network-config")
                .map_err(|e| CloudInitError::Fat(e.to_string()))?
                .write_all(nc.as_bytes())?;
        }
    } // FileSystem dropped here, flushing all writes to `buf`.

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn seed_image_contains_expected_files() {
        let buf = build_seed_image(
            "#cloud-config\npackages:\n  - curl\n",
            "instance-id: test-vm\nlocal-hostname: test\n",
            Some("version: 1\n"),
        )
        .unwrap();

        assert_eq!(buf.len(), SEED_IMAGE_SIZE);

        let cursor = Cursor::new(buf);
        let fs = fatfs::FileSystem::new(cursor, fatfs::FsOptions::new()).unwrap();

        // fatfs trims trailing spaces from volume_label()
        assert_eq!(fs.volume_label().trim(), "CIDATA");

        let root = fs.root_dir();
        for name in &["meta-data", "user-data", "network-config"] {
            let mut f = root.open_file(name).unwrap();
            let mut content = String::new();
            f.read_to_string(&mut content).unwrap();
            assert!(!content.is_empty(), "{name} should not be empty");
        }
    }
}
