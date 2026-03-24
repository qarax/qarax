use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::mount::{MsFlags, mount};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, fork};
use serde::Deserialize;

#[derive(Deserialize, Default)]
struct Config {
    #[serde(default)]
    entrypoint: Vec<String>,
    #[serde(default)]
    cmd: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
}

fn log(msg: impl AsRef<str>) {
    let line = format!("<6>qarax-init: {}\n", msg.as_ref());
    let _ = std::fs::write("/dev/kmsg", line.as_bytes());
    eprintln!("qarax-init: {}", msg.as_ref());
}

fn try_mount(source: &str, target: &str, fstype: &str) {
    let result = mount(
        Some(source),
        target,
        Some(fstype),
        MsFlags::empty(),
        None::<&str>,
    );
    if let Err(e) = result
        && e != nix::errno::Errno::EBUSY
    {
        log(format!("warning: mount {fstype} on {target} failed: {e}"));
    }
}

/// Bring up the loopback interface via SIOCGIFFLAGS / SIOCSIFFLAGS ioctls.
/// Does not rely on `ip` or any other userspace tool, so it works in
/// scratch/distroless images.
fn setup_loopback() {
    let fd = match nix::sys::socket::socket(
        nix::sys::socket::AddressFamily::Inet,
        nix::sys::socket::SockType::Datagram,
        nix::sys::socket::SockFlag::empty(),
        None,
    ) {
        Ok(f) => f,
        Err(e) => {
            log(format!("warning: loopback setup: socket: {e}"));
            return;
        }
    };

    let mut ifreq: libc::ifreq = unsafe { std::mem::zeroed() };
    // Write "lo\0" into ifr_name
    let name = b"lo\0";
    unsafe {
        std::ptr::copy_nonoverlapping(
            name.as_ptr() as *const libc::c_char,
            ifreq.ifr_name.as_mut_ptr(),
            name.len(),
        );

        if libc::ioctl(
            fd.as_raw_fd(),
            libc::SIOCGIFFLAGS as libc::c_int,
            &mut ifreq,
        ) < 0
        {
            log(format!(
                "warning: loopback SIOCGIFFLAGS: {}",
                std::io::Error::last_os_error()
            ));
            return;
        }

        ifreq.ifr_ifru.ifru_flags |= libc::IFF_UP as libc::c_short;

        if libc::ioctl(
            fd.as_raw_fd(),
            libc::SIOCSIFFLAGS as libc::c_int,
            &mut ifreq,
        ) < 0
        {
            log(format!(
                "warning: loopback SIOCSIFFLAGS: {}",
                std::io::Error::last_os_error()
            ));
            return;
        }
    }

    log("loopback interface up");
}

fn parse_cmdline_param(key: &str) -> Option<String> {
    let cmdline = std::fs::read_to_string("/proc/cmdline").ok()?;
    cmdline
        .split_whitespace()
        .find_map(|param| param.strip_prefix(key).map(|value| value.to_string()))
}

/// Parse `root=DEVICE` from /proc/cmdline.
fn parse_root_device() -> Option<String> {
    parse_cmdline_param("root=")
}

/// Parse `rootfstype=TYPE` from /proc/cmdline, defaulting to ext4.
fn parse_root_fstype() -> String {
    parse_cmdline_param("rootfstype=").unwrap_or_else(|| "ext4".to_string())
}

/// When running from an initramfs, mount the root device and switch_root into it.
/// Returns true if we switched root, false if already on the real rootfs.
fn maybe_switch_root() -> bool {
    // The test initramfs also carries /.qarax-config.json, so use the injected
    // /.qarax-init marker to detect whether we're already on the real rootfs.
    if std::path::Path::new("/.qarax-init").exists() {
        return false;
    }

    let root_dev = match parse_root_device() {
        Some(dev) => dev,
        None => {
            log("no root= on cmdline, staying on initramfs");
            return false;
        }
    };
    let root_fstype = parse_root_fstype();

    log(format!("switching root to {root_dev} ({root_fstype})"));

    // Create mount point
    let _ = std::fs::create_dir_all("/newroot");

    // Mount the root device
    if let Err(e) = mount(
        Some(root_dev.as_str()),
        "/newroot",
        Some(root_fstype.as_str()),
        MsFlags::empty(),
        None::<&str>,
    ) {
        log(format!(
            "failed to mount {root_dev} ({root_fstype}) on /newroot: {e}"
        ));
        return false;
    }

    // Move pseudo-filesystems to the new root
    for (src, dst) in [
        ("/dev", "/newroot/dev"),
        ("/proc", "/newroot/proc"),
        ("/sys", "/newroot/sys"),
    ] {
        if std::path::Path::new(dst).exists()
            && let Err(e) = mount(Some(src), dst, None::<&str>, MsFlags::MS_MOVE, None::<&str>)
        {
            log(format!("warning: move mount {src} -> {dst}: {e}"));
        }
    }

    // switch_root: chdir, mount --move newroot to /, chroot
    if let Err(e) = std::env::set_current_dir("/newroot") {
        log(format!("chdir /newroot failed: {e}"));
        return false;
    }

    if let Err(e) = mount(Some("."), "/", None::<&str>, MsFlags::MS_MOVE, None::<&str>) {
        log(format!("mount --move . / failed: {e}"));
        return false;
    }

    let dot = std::ffi::CString::new(".").unwrap();
    if unsafe { libc::chroot(dot.as_ptr()) } != 0 {
        log(format!(
            "chroot failed: {}",
            std::io::Error::last_os_error()
        ));
        return false;
    }

    let _ = std::env::set_current_dir("/");

    log("switch_root complete");
    true
}

fn main() {
    try_mount("proc", "/proc", "proc");
    try_mount("sysfs", "/sys", "sysfs");
    try_mount("devtmpfs", "/dev", "devtmpfs");

    // Create standard fd symlinks expected by many programs
    let _ = std::os::unix::fs::symlink("/proc/self/fd", "/dev/fd");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/0", "/dev/stdin");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/1", "/dev/stdout");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/2", "/dev/stderr");

    // If running from initramfs, mount the real root and switch into it.
    maybe_switch_root();

    setup_loopback();

    let config: Config = std::fs::read_to_string("/.qarax-config.json")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    for var in &config.env {
        if let Some((key, val)) = var.split_once('=') {
            // SAFETY: single-threaded before fork
            unsafe { std::env::set_var(key, val) };
        }
    }

    let mut argv = config.entrypoint;
    argv.extend(config.cmd);
    if argv.is_empty() {
        argv.push("/bin/sh".to_string());
    }

    log(format!("starting: {}", argv.join(" ")));

    // Fork so PID 1 stays as the init loop (reaping zombies) while the
    // child exec's the workload. This avoids the PID 1 signal-handling
    // quirk where SIGTERM/SIGINT are ignored by default.
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            let err = Command::new(&argv[0]).args(&argv[1..]).exec();
            log(format!("failed to exec '{}': {}", argv[0], err));
            std::process::exit(1);
        }

        Ok(ForkResult::Parent { child }) => {
            // Block-wait for any child. Reap zombies as they arrive;
            // exit with the workload's exit code when it finishes.
            loop {
                match waitpid(nix::unistd::Pid::from_raw(-1), None) {
                    Ok(WaitStatus::Exited(pid, code)) if pid == child => {
                        std::process::exit(code);
                    }
                    Ok(WaitStatus::Signaled(pid, sig, _)) if pid == child => {
                        // Propagate signal as exit code (Unix convention: 128 + signum)
                        std::process::exit(128 + sig as i32);
                    }
                    Ok(_) => continue, // orphaned zombie reaped, keep going
                    Err(nix::errno::Errno::ECHILD) => std::process::exit(0),
                    Err(e) => {
                        log(format!("waitpid: {e}"));
                        std::process::exit(1);
                    }
                }
            }
        }

        Err(e) => {
            log(format!("fork failed: {e}"));
            std::process::exit(1);
        }
    }
}
