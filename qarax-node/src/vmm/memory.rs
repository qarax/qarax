//! The memory each VM holds of its own, which the control plane charges a
//! fork for: a fork maps its source's memory image copy-on-write, and holds
//! only the pages it writes.

use std::{io, path::Path};

use tokio::net::UnixStream;

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
}
