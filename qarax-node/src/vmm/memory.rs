//! The memory each VM holds of its own, which the control plane charges a
//! fork for: a fork maps its source's memory image copy-on-write, and holds
//! only the pages it writes. And the pages of its memory image a fork has
//! touched, which the next fork of its source reads ahead.

use std::io::{self, SeekFrom};
use std::ops::Range;
use std::path::Path;

use nix::libc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::UnixStream;

/// A `/proc/<pid>/pagemap` entry's bit for a page in the process's page tables.
const PAGEMAP_PRESENT: u64 = 1 << 63;
/// How many pagemap entries to read at a time: a 256 MiB guest's.
const PAGEMAP_CHUNK: usize = 64 * 1024;

/// The memory the hypervisor serving its API on `api_socket` holds of its
/// own (see [`private_bytes`]).
pub async fn private_memory(api_socket: &Path) -> io::Result<u64> {
    let pid = listening_pid(api_socket).await?;
    let status = tokio::fs::read_to_string(format!("/proc/{pid}/status")).await?;
    private_bytes(&status)
        .ok_or_else(|| io::Error::other(format!("/proc/{pid}/status has no memory counters")))
}

/// The process listening on `socket`: the peer of a connection to it, whether
/// this node started the process or recovered it.
async fn listening_pid(socket: &Path) -> io::Result<i32> {
    UnixStream::connect(socket)
        .await?
        .peer_cred()?
        .pid()
        .ok_or_else(|| io::Error::other(format!("{} has no peer PID", socket.display())))
}

/// The parts of `file` that process `pid` has in its page tables through
/// its mappings of the file, as byte ranges of the file: the pages it touched
/// since it mapped them, and the neighbours the kernel mapped along with them.
pub async fn mapped_pages(pid: u32, file: &Path) -> io::Result<Vec<Range<u64>>> {
    // The kernel names a mapped file by its path with symlinks resolved.
    let file = tokio::fs::canonicalize(file).await?;
    let maps = tokio::fs::read_to_string(format!("/proc/{pid}/maps")).await?;
    let mut pagemap = tokio::fs::File::open(format!("/proc/{pid}/pagemap")).await?;
    // SAFETY: sysconf only reads a constant.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    let mut pages = Vec::new();
    // One 64-bit entry for each page of the address space, read a chunk at a
    // time.
    let mut entries = vec![0; PAGEMAP_CHUNK * 8];
    for mapping in mappings_of(&maps, &file) {
        let count = (mapping.end - mapping.start) / page;
        pagemap
            .seek(SeekFrom::Start(mapping.start / page * 8))
            .await?;
        let mut read = 0;
        while read < count {
            let chunk = &mut entries[..(count - read).min(PAGEMAP_CHUNK as u64) as usize * 8];
            pagemap.read_exact(chunk).await?;
            let (chunk, _) = chunk.as_chunks::<8>();
            pages.extend(
                chunk
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| u64::from_ne_bytes(**entry) & PAGEMAP_PRESENT != 0)
                    .map(|(i, _)| mapping.offset + (read + i as u64) * page),
            );
            read += chunk.len() as u64;
        }
    }
    pages.sort_unstable();
    let mut ranges: Vec<Range<u64>> = Vec::new();
    for start in pages {
        match ranges.last_mut() {
            Some(last) if last.end >= start => last.end = last.end.max(start + page),
            _ => ranges.push(start..start + page),
        }
    }
    Ok(ranges)
}

/// A mapping of a file in a process's address space: `start..end` maps the
/// file from `offset` on.
struct Mapping {
    start: u64,
    end: u64,
    offset: u64,
}

/// `file`'s mappings in `maps`, a `/proc/<pid>/maps`: lines of
/// `start-end perms offset dev inode path`, in hex but for the inode.
fn mappings_of<'a>(maps: &'a str, file: &'a Path) -> impl Iterator<Item = Mapping> + 'a {
    maps.lines().filter_map(move |line| {
        let mut fields = line.splitn(6, ' ');
        let (start, end) = fields.next()?.split_once('-')?;
        let offset = fields.nth(1)?;
        if Path::new(fields.nth(2)?.trim_start()) != file {
            return None;
        }
        let hex = |field: &str| u64::from_str_radix(field, 16).ok();
        Some(Mapping {
            start: hex(start)?,
            end: hex(end)?,
            offset: hex(offset)?,
        })
    })
}

/// A process's anonymous, shared memory and huge pages, from its
/// `/proc/<pid>/status`: its memory but the file pages, which the kernel
/// drops and reads back. A fork's pages it hasn't written are file pages of
/// its source's memory image.
fn private_bytes(status: &str) -> Option<u64> {
    let kib = |field: &str| {
        status.lines().find_map(|line| {
            line.strip_prefix(field)?
                .strip_prefix(':')?
                .trim()
                .strip_suffix(" kB")?
                .parse::<u64>()
                .ok()
        })
    };
    Some((kib("RssAnon")? + kib("RssShmem")? + kib("HugetlbPages")?) * 1024)
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::FileExt;

    use super::*;

    #[test]
    fn private_bytes_leaves_out_file_pages() {
        let status = "Name:\tfirecracker\n\
                      VmRSS:\t  300000 kB\n\
                      RssAnon:\t    7000 kB\n\
                      RssFile:\t  262000 kB\n\
                      RssShmem:\t    1000 kB\n\
                      HugetlbPages:\t   2048 kB\n";
        assert_eq!(private_bytes(status), Some((7000 + 1000 + 2048) * 1024));
        assert_eq!(private_bytes("Name:\tfirecracker\n"), None);
    }

    #[tokio::test]
    async fn an_api_socket_leads_to_the_process_listening_on_it() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("api.sock");
        let _listener = tokio::net::UnixListener::bind(&socket).unwrap();
        assert_eq!(
            listening_pid(&socket).await.unwrap(),
            std::process::id() as i32
        );
        assert!(private_memory(&socket).await.unwrap() > 0);
    }

    #[tokio::test]
    async fn the_pages_a_process_touched_of_a_file_it_maps() {
        // SAFETY: sysconf only reads a constant.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory");
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        // Mapped from its page 32 on, privately, as a VMM maps a memory image,
        // and longer than a chunk of pagemap entries.
        let (from, pages) = (32, PAGEMAP_CHUNK + 1024);
        file.set_len(((from + pages) * page) as u64).unwrap();
        let touched = [40, from + PAGEMAP_CHUNK + 100];
        for page_index in touched {
            file.write_all_at(&[1], (page_index * page) as u64).unwrap();
        }
        let len = pages * page;
        // SAFETY: a new mapping of an open file, unmapped below.
        let mapped = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                (from * page) as libc::off_t,
            )
        };
        assert_ne!(mapped, libc::MAP_FAILED);
        for page_index in touched {
            // SAFETY: within the mapping.
            let byte = unsafe {
                mapped
                    .cast::<u8>()
                    .add((page_index - from) * page)
                    .read_volatile()
            };
            assert_eq!(byte, 1);
        }

        let ranges = mapped_pages(std::process::id(), &path).await.unwrap();
        // SAFETY: the mapping made above, which nothing uses any more.
        unsafe { libc::munmap(mapped, len) };

        let page = page as u64;
        for page_index in touched {
            let offset = page_index as u64 * page;
            assert!(
                ranges.iter().any(|range| range.contains(&offset)),
                "{ranges:?} lacks page {page_index}"
            );
        }
        // The kernel may map neighbours along, but only of the mapped part.
        let mapped_part = from as u64 * page..(from + pages) as u64 * page;
        assert!(
            ranges
                .iter()
                .all(|range| range.start >= mapped_part.start && range.end <= mapped_part.end),
            "{ranges:?}"
        );

        let other = dir.path().join("other");
        std::fs::write(&other, b"unmapped").unwrap();
        assert!(
            mapped_pages(std::process::id(), &other)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
