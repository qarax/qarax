//! Disk files a VM writes to: written back to disk, and copied.

use std::fs::{File, OpenOptions};
use std::io;
use std::ops::Range;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;
use std::path::Path;

use nix::fcntl::{PosixFadviseAdvice, posix_fadvise};
use nix::libc;

/// Write back the dirty pages of the file at `path` and wait for them,
/// without asking the device to flush its cache: a reflink copy needs the data
/// written, not durable, and a flush can take seconds where a disk is slow.
pub fn write_back(path: &Path) -> io::Result<()> {
    let file = File::open(path)?;
    let flags = libc::SYNC_FILE_RANGE_WAIT_BEFORE
        | libc::SYNC_FILE_RANGE_WRITE
        | libc::SYNC_FILE_RANGE_WAIT_AFTER;
    // SAFETY: the descriptor is open for the whole call.
    if unsafe { libc::sync_file_range(file.as_raw_fd(), 0, 0, flags) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Copy the regular file `src` to `dst`, a new file, as it is: its data
/// extents only, so a sparse file stays sparse. `copy_file_range` shares the
/// data where the filesystem has reflinks (XFS, btrfs), so the copy takes
/// milliseconds whatever the file's size; elsewhere, and between two
/// filesystems, it takes as long as copying the data.
pub fn copy_sparse(src: &Path, dst: &Path) -> io::Result<()> {
    let src = File::open(src)?;
    let len = regular_file_len(&src)?;
    let dst = OpenOptions::new().write(true).create_new(true).open(dst)?;
    dst.set_len(len)?;
    copy_extents(&src, &dst, len)
}

/// Write the data extents of the regular file `src` over `dst`, a file of
/// the same size, at the same offsets: `dst` keeps what is in `src`'s holes.
/// Lays the pages of a Firecracker diff snapshot over a copy of the memory
/// image it is a diff of.
///
/// The data goes through the page cache, to be written back later, rather
/// than through `copy_file_range`: on XFS that reflinks each extent, which
/// first writes back that part of `src` (Firecracker leaves its snapshot
/// files in the page cache) and logs a transaction for each, far slower for a
/// diff of many small extents. This way the diff, deleted once laid over,
/// needn't reach the disk at all.
pub fn overlay_extents(src: &Path, dst: &Path) -> io::Result<()> {
    let src = File::open(src)?;
    let len = regular_file_len(&src)?;
    let dst = OpenOptions::new().write(true).open(dst)?;
    if dst.metadata()?.len() != len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the files' sizes differ",
        ));
    }
    write_extents(&src, &dst, &data_extents(&src, len)?)
}

/// Copy `extents` of `src` to the same offsets of `dst`, through the page
/// cache.
fn write_extents(src: &File, dst: &File, extents: &[Range<u64>]) -> io::Result<()> {
    let mut buf = vec![0; COPY_CHUNK];
    for extent in extents {
        let mut offset = extent.start;
        while offset < extent.end {
            let chunk = &mut buf[..(extent.end - offset).min(COPY_CHUNK as u64) as usize];
            src.read_exact_at(chunk, offset)?;
            dst.write_all_at(chunk, offset)?;
            offset += chunk.len() as u64;
        }
    }
    Ok(())
}

/// How much of a file [`write_extents`] copies at a time.
const COPY_CHUNK: usize = 1024 * 1024;

/// Start reading `ranges` of `file` (byte ranges) into the page cache. The
/// kernel queues the reads and returns, but may wait for room in the
/// device's queue first.
pub fn read_ahead(file: &File, ranges: &[Range<u64>]) -> io::Result<()> {
    for range in ranges {
        let offset = i64::try_from(range.start).map_err(io::Error::other)?;
        let len = i64::try_from(range.end - range.start).map_err(io::Error::other)?;
        posix_fadvise(
            file.as_raw_fd(),
            offset,
            len,
            PosixFadviseAdvice::POSIX_FADV_WILLNEED,
        )?;
    }
    Ok(())
}

fn regular_file_len(file: &File) -> io::Result<u64> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(metadata.len())
}

/// `file`'s data extents within its first `len` bytes, in order.
fn data_extents(file: &File, len: u64) -> io::Result<Vec<Range<u64>>> {
    let len = i64::try_from(len).map_err(io::Error::other)?;
    let mut extents = Vec::new();
    let mut offset = 0;
    while offset < len {
        // SAFETY: the descriptor is open for the whole call.
        let data = unsafe { libc::lseek(file.as_raw_fd(), offset, libc::SEEK_DATA) };
        if data < 0 {
            let error = io::Error::last_os_error();
            // No data from `offset` on.
            if error.raw_os_error() == Some(libc::ENXIO) {
                break;
            }
            return Err(error);
        }
        // SAFETY: as above.
        let hole = unsafe { libc::lseek(file.as_raw_fd(), data, libc::SEEK_HOLE) };
        if hole < 0 {
            return Err(io::Error::last_os_error());
        }
        extents.push(data as u64..hole as u64);
        offset = hole;
    }
    Ok(extents)
}

/// Copy `src`'s data extents, within its first `len` bytes, to the same
/// offsets of `dst`: in the kernel, or through the page cache between
/// filesystems the kernel doesn't copy between.
fn copy_extents(src: &File, dst: &File, len: u64) -> io::Result<()> {
    let extents = data_extents(src, len)?;
    match copy_file_ranges(src, dst, &extents) {
        // `copy_file_range` refuses files on two filesystems (EXDEV) before it
        // copies anything. A snapshot's copies of its VM's disks go to its
        // storage pool, which needn't be on the disks' filesystem.
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => write_extents(src, dst, &extents),
        result => result,
    }
}

/// Copy `extents` of `src` to the same offsets of `dst` with
/// `copy_file_range`.
fn copy_file_ranges(src: &File, dst: &File, extents: &[Range<u64>]) -> io::Result<()> {
    for extent in extents {
        let (mut from, mut to) = (extent.start as i64, extent.start as i64);
        let hole = extent.end as i64;
        while from < hole {
            // SAFETY: both descriptors are open, and the offsets are valid
            // pointers the call advances.
            let copied = unsafe {
                libc::copy_file_range(
                    src.as_raw_fd(),
                    &mut from,
                    dst.as_raw_fd(),
                    &mut to,
                    (hole - from) as usize,
                    0,
                )
            };
            match copied {
                n if n < 0 => return Err(io::Error::last_os_error()),
                0 => break,
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::fs::MetadataExt;

    use super::*;

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn a_sparse_copy_has_the_files_data_and_its_holes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("disk");
        let mut file = File::create(&src).unwrap();
        file.set_len(64 * MIB).unwrap();
        file.write_all(b"head").unwrap();
        file.seek(SeekFrom::Start(32 * MIB)).unwrap();
        file.write_all(b"middle").unwrap();
        file.sync_all().unwrap();

        let dst = dir.path().join("copy");
        copy_sparse(&src, &dst).unwrap();

        assert_eq!(std::fs::read(&dst).unwrap(), std::fs::read(&src).unwrap());
        // Far from the 64 MiB a full copy would take up.
        assert!(std::fs::metadata(&dst).unwrap().blocks() * 512 < 8 * MIB);
    }

    #[test]
    fn an_overlay_writes_the_extents_over_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let (base, diff) = (dir.path().join("base"), dir.path().join("diff"));
        std::fs::write(&base, vec![1u8; 2 * MIB as usize]).unwrap();
        let mut file = File::create(&diff).unwrap();
        file.set_len(2 * MIB).unwrap();
        file.seek(SeekFrom::Start(MIB)).unwrap();
        file.write_all(&[2u8; 4096]).unwrap();
        file.sync_all().unwrap();

        overlay_extents(&diff, &base).unwrap();

        let merged = std::fs::read(&base).unwrap();
        let page = MIB as usize..MIB as usize + 4096;
        assert!(merged[page.clone()].iter().all(|b| *b == 2));
        assert!(merged[..page.start].iter().all(|b| *b == 1));
        assert!(merged[page.end..].iter().all(|b| *b == 1));

        let short = dir.path().join("short");
        std::fs::write(&short, b"x").unwrap();
        assert_eq!(
            overlay_extents(&diff, &short).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_copy_crosses_filesystems() {
        // /dev/shm is a tmpfs of its own, where copy_file_range from the
        // temporary directory's filesystem fails with EXDEV.
        let (from, to) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir_in("/dev/shm").unwrap(),
        );
        let src = from.path().join("disk");
        let mut file = File::create(&src).unwrap();
        file.set_len(4 * MIB).unwrap();
        file.seek(SeekFrom::Start(MIB)).unwrap();
        file.write_all(b"data").unwrap();

        let dst = to.path().join("copy");
        copy_sparse(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), std::fs::read(&src).unwrap());
    }

    #[test]
    fn a_copy_never_overwrites_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("disk"), dir.path().join("copy"));
        std::fs::write(&src, b"disk").unwrap();
        std::fs::write(&dst, b"other").unwrap();

        let error = copy_sparse(&src, &dst).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&dst).unwrap(), b"other");
    }
}
