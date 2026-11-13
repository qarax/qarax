//! Cloud-init NoCloud seed image generation.
//!
//! Produces a small VFAT disk image (labeled "CIDATA") containing the three
//! standard NoCloud files: `meta-data`, `user-data`, and optionally
//! `network-config`. Both Cloud Hypervisor and Firecracker expose this as a
//! read-only virtio-blk device; cloud-init's NoCloud datasource discovers it
//! by the volume label.

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

    fn read_seed(buf: Vec<u8>) -> (String, Vec<(String, String)>) {
        let fs = fatfs::FileSystem::new(Cursor::new(buf), fatfs::FsOptions::new()).unwrap();
        // fatfs keeps the label's space padding.
        let label = fs.volume_label().trim().to_string();
        let mut files = Vec::new();
        for entry in fs.root_dir().iter() {
            let entry = entry.unwrap();
            let mut content = String::new();
            entry.to_file().read_to_string(&mut content).unwrap();
            files.push((entry.file_name(), content));
        }
        files.sort();
        (label, files)
    }

    #[test]
    fn seed_image_holds_each_file_under_its_nocloud_name() {
        let user_data = "#cloud-config\npackages:\n  - curl\n";
        let meta_data = "instance-id: test-vm\nlocal-hostname: test\n";
        let network_config = "version: 1\n";

        let buf = build_seed_image(user_data, meta_data, Some(network_config)).unwrap();
        assert_eq!(buf.len(), SEED_IMAGE_SIZE);
        let (label, files) = read_seed(buf);
        assert_eq!(label, "CIDATA");
        assert_eq!(
            files,
            [
                ("meta-data".to_string(), meta_data.to_string()),
                ("network-config".to_string(), network_config.to_string()),
                ("user-data".to_string(), user_data.to_string()),
            ]
        );

        let (_, files) = read_seed(build_seed_image(user_data, meta_data, None).unwrap());
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["meta-data", "user-data"]);
    }
}
