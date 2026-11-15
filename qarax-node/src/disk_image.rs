//! Disk image formats.

use std::io::{ErrorKind, Read};
use std::path::Path;

use crate::rpc::node::ImageFormat;

/// The format of the image at `path` by its first bytes: qcow2 if they are
/// the qcow2 magic, raw otherwise.
///
/// A guest can write the start of its raw disk, so this is only trustworthy
/// before a guest has used the image; after that, the format recorded on the
/// disk's storage object decides.
pub fn probe_image_format(path: &Path) -> std::io::Result<ImageFormat> {
    let mut magic = [0u8; 4];
    match std::fs::File::open(path)?.read_exact(&mut magic) {
        Ok(()) if &magic == b"QFI\xfb" => Ok(ImageFormat::Qcow2),
        Ok(()) => Ok(ImageFormat::Raw),
        // Too short to hold the magic.
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(ImageFormat::Raw),
        Err(e) => Err(e),
    }
}

/// `format`, or for one not recorded (sent by a control plane older than
/// the node) the image's, by its first bytes.
pub fn recorded_or_probed(format: ImageFormat, path: &Path) -> ImageFormat {
    match format {
        ImageFormat::Unspecified => match probe_image_format(path) {
            Ok(ImageFormat::Qcow2) => ImageFormat::Qcow2,
            _ => ImageFormat::Raw,
        },
        format => format,
    }
}

/// Grow the image at `path` to `new_size` bytes in place, sparsely. Uses
/// `qemu-img`, which also updates a qcow2 image's virtual size, kept in its
/// header; growing the file alone would leave the disk its old size.
pub async fn grow_image(path: &Path, new_size: i64, format: ImageFormat) -> Result<(), String> {
    let format = match recorded_or_probed(format, path) {
        ImageFormat::Qcow2 => "qcow2",
        _ => "raw",
    };
    let output = tokio::process::Command::new("qemu-img")
        .args(["resize", "-f", format])
        .arg(path)
        .arg(new_size.to_string())
        .output()
        .await
        .map_err(|e| format!("failed to run qemu-img: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "qemu-img resize {} failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reads_the_qcow2_magic_and_fails_on_a_missing_image() {
        let directory = tempfile::TempDir::new().unwrap();
        for (contents, expected) in [
            (&b"QFI\xfb\0\0\0\x03"[..], ImageFormat::Qcow2),
            (&b"\0\0\0\0\0\0\0\0"[..], ImageFormat::Raw),
            (&b"QF"[..], ImageFormat::Raw),
        ] {
            let image = directory.path().join("disk.img");
            std::fs::write(&image, contents).unwrap();
            assert_eq!(
                probe_image_format(&image).unwrap(),
                expected,
                "{contents:?}"
            );
        }
        // Recording "raw" for an image that isn't there yet would be wrong.
        let missing = probe_image_format(&directory.path().join("missing.img")).unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::NotFound);
    }
}
