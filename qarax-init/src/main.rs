use std::collections::HashMap;
use std::ffi::CString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex, OnceLock,
    mpsc::{self, Receiver, Sender},
};
use std::thread;
use std::time::{Duration, Instant};

use nix::mount::{MsFlags, mount};
use nix::sys::signal::{Signal, kill};
use nix::sys::socket::{
    AddressFamily, Backlog, SockFlag, SockType, VsockAddr, accept4, bind, listen, socket,
};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, Pid, fork};
use serde::{Deserialize, Serialize};

const SANDBOX_EXEC_PORT: u32 = 7000;
const MAX_EXEC_REQUEST_BYTES: u64 = 1024 * 1024;
const MAX_COMPLETED_EXIT_RECORDS: usize = 128;
const MAX_COMPLETED_EXIT_AGE: Duration = Duration::from_secs(30);

/// How long processes get between SIGTERM and SIGKILL when the VM shuts down,
/// as with `docker stop`. Below qarax-node's graceful stop timeout, so the
/// guest powers itself off before the node forces it.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
// From linux/input-event-codes.h.
const EV_KEY: u16 = 0x01;
const KEY_POWER: u16 = 116;

/// How a shutdown ends.
#[derive(Clone, Copy, Debug)]
enum Ending {
    PowerOff,
    /// For a Firecracker guest, which can't power off, only halt, but whose
    /// VMM exits when it reboots.
    Reboot,
}

/// Set, to how it ends, once a shutdown starts. From then on the main loop
/// keeps reaping when the workload exits, and ends the VM once nothing is left.
static SHUTDOWN: OnceLock<Ending> = OnceLock::new();

/// A guest agent request other than running a command, on its own line:
/// `{"shutdown": {"reboot": false}}`.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ControlRequest {
    /// Stop every process, then power off or reboot, as the power button
    /// does. qarax-node sends it to Firecracker guests, which have no power
    /// button.
    Shutdown {
        #[serde(default)]
        reboot: bool,
    },
    /// This guest is a fork: it resumed from another VM's snapshot, with
    /// that VM's random generator state and clock. Mix in `entropy` (hex,
    /// fresh from the node), reseed the kernel's generator, and set the clock
    /// to `unix_time_ns`.
    Forked { entropy: String, unix_time_ns: u64 },
    /// qarax-node asks whether the agent is up, before it reports a sandbox
    /// ready.
    Ping {},
    /// This guest is a fork with NICs of its own, which still have their
    /// source's addresses: give each the fork's.
    Readdress(Vec<NicAddress>),
}

/// A NIC of a fork's guest, found by its source's MAC (`from`), and the
/// fork's MAC and IPv4 address for it.
#[derive(Deserialize)]
struct NicAddress {
    from: String,
    mac: String,
    ip: Ipv4Addr,
    mask: Ipv4Addr,
}

#[derive(Deserialize, Default)]
struct Config {
    #[serde(default)]
    entrypoint: Vec<String>,
    #[serde(default)]
    cmd: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
}

#[derive(Deserialize)]
struct ExecRequest {
    command: Vec<String>,
    timeout_secs: Option<u64>,
}

#[derive(Serialize)]
struct ExecResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

struct CompletedExit {
    exit_code: i32,
    recorded_at: Instant,
}

#[derive(Default)]
struct ExitRegistryState {
    waiters: HashMap<Pid, Sender<i32>>,
    completed: HashMap<Pid, CompletedExit>,
    pending_registrations: usize,
}

type ExitRegistry = Arc<Mutex<ExitRegistryState>>;

fn prune_completed_exits(registry: &mut ExitRegistryState) {
    let now = Instant::now();
    registry
        .completed
        .retain(|_, completed| now.duration_since(completed.recorded_at) <= MAX_COMPLETED_EXIT_AGE);

    if registry.completed.len() <= MAX_COMPLETED_EXIT_RECORDS {
        return;
    }

    let mut completed: Vec<_> = registry
        .completed
        .iter()
        .map(|(pid, completed)| (*pid, completed.recorded_at))
        .collect();
    completed.sort_by_key(|(_, recorded_at)| *recorded_at);

    let to_remove = completed.len() - MAX_COMPLETED_EXIT_RECORDS;
    for (pid, _) in completed.into_iter().take(to_remove) {
        registry.completed.remove(&pid);
    }
}

fn begin_exec_registration(exit_registry: &ExitRegistry) {
    let mut registry = exit_registry.lock().unwrap();
    registry.pending_registrations += 1;
    prune_completed_exits(&mut registry);
}

fn register_exec_waiter(exit_registry: &ExitRegistry, pid: Pid, tx: Sender<i32>) {
    let mut registry = exit_registry.lock().unwrap();
    registry.pending_registrations = registry.pending_registrations.saturating_sub(1);
    prune_completed_exits(&mut registry);

    if let Some(completed) = registry.completed.remove(&pid) {
        let _ = tx.send(completed.exit_code);
    } else {
        registry.waiters.insert(pid, tx);
    }
}

fn cancel_exec_registration(exit_registry: &ExitRegistry) {
    let mut registry = exit_registry.lock().unwrap();
    registry.pending_registrations = registry.pending_registrations.saturating_sub(1);
    prune_completed_exits(&mut registry);
}

fn record_child_exit(exit_registry: &ExitRegistry, pid: Pid, exit_code: i32) {
    let mut registry = exit_registry.lock().unwrap();
    prune_completed_exits(&mut registry);

    if let Some(tx) = registry.waiters.remove(&pid) {
        let _ = tx.send(exit_code);
    } else if registry.pending_registrations > 0 {
        registry.completed.insert(
            pid,
            CompletedExit {
                exit_code,
                recorded_at: Instant::now(),
            },
        );
    }
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

fn parse_root_device() -> Option<String> {
    parse_cmdline_param("root=")
}

fn parse_root_fstype() -> String {
    parse_cmdline_param("rootfstype=").unwrap_or_else(|| "ext4".to_string())
}

fn maybe_switch_root() -> bool {
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

    let _ = std::fs::create_dir_all("/newroot");
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

fn spawn_reader<R: Read + Send + 'static>(mut reader: R) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        let _ = tx.send(String::from_utf8_lossy_owned(buf));
    });
    rx
}

fn serve_exec_connection(stream_fd: OwnedFd, exit_registry: ExitRegistry) {
    let mut stream = std::os::unix::net::UnixStream::from(stream_fd);

    let mut request_line = String::new();
    let request: ExecRequest = match BufReader::new((&mut stream).take(MAX_EXEC_REQUEST_BYTES + 1))
        .read_line(&mut request_line)
    {
        Ok(0) => {
            log("sandbox exec request stream closed before payload");
            return;
        }
        Ok(_) if !request_line.ends_with('\n') => {
            let body = serde_json::to_vec(&ExecResponse {
                exit_code: 2,
                stdout: String::new(),
                stderr: format!(
                    "invalid exec request: request exceeds {} bytes or is missing a newline terminator",
                    MAX_EXEC_REQUEST_BYTES
                ),
                timed_out: false,
            })
            .unwrap_or_default();
            let _ = stream.write_all(&body);
            return;
        }
        Ok(_) => {
            match serde_json::from_str(&request_line) {
                Ok(ControlRequest::Shutdown { reboot }) => {
                    let body = serde_json::to_vec(&ExecResponse {
                        exit_code: 0,
                        stdout: String::new(),
                        stderr: String::new(),
                        timed_out: false,
                    })
                    .unwrap_or_default();
                    let _ = stream.write_all(&body);
                    drop(stream);
                    let ending = if reboot {
                        Ending::Reboot
                    } else {
                        Ending::PowerOff
                    };
                    shut_down("qarax-node asked to shut down", ending);
                    return;
                }
                Ok(ControlRequest::Ping {}) => {
                    let body = serde_json::to_vec(&ExecResponse {
                        exit_code: 0,
                        stdout: String::new(),
                        stderr: String::new(),
                        timed_out: false,
                    })
                    .unwrap_or_default();
                    let _ = stream.write_all(&body);
                    return;
                }
                Ok(ControlRequest::Readdress(nics)) => {
                    let (exit_code, stderr) = match readdress(&nics) {
                        Ok(()) => {
                            log("forked: took the fork's network addresses");
                            (0, String::new())
                        }
                        Err(e) => {
                            log(format!("taking the fork's network addresses failed: {e}"));
                            (1, e)
                        }
                    };
                    let body = serde_json::to_vec(&ExecResponse {
                        exit_code,
                        stdout: String::new(),
                        stderr,
                        timed_out: false,
                    })
                    .unwrap_or_default();
                    let _ = stream.write_all(&body);
                    return;
                }
                Ok(ControlRequest::Forked {
                    entropy,
                    unix_time_ns,
                }) => {
                    let (exit_code, stderr) = match resume_as_fork(&entropy, unix_time_ns) {
                        Ok(()) => {
                            log("forked: reseeded the random generator and set the clock");
                            (0, String::new())
                        }
                        Err(e) => {
                            log(format!("fork notice failed: {e}"));
                            (1, e)
                        }
                    };
                    let body = serde_json::to_vec(&ExecResponse {
                        exit_code,
                        stdout: String::new(),
                        stderr,
                        timed_out: false,
                    })
                    .unwrap_or_default();
                    let _ = stream.write_all(&body);
                    return;
                }
                Err(_) => {}
            }
            match serde_json::from_str(&request_line) {
                Ok(req) => req,
                Err(e) => {
                    let body = serde_json::to_vec(&ExecResponse {
                        exit_code: 2,
                        stdout: String::new(),
                        stderr: format!("invalid exec request: {e}"),
                        timed_out: false,
                    })
                    .unwrap_or_default();
                    let _ = stream.write_all(&body);
                    return;
                }
            }
        }
        Err(e) => {
            log(format!("sandbox exec read failed: {e}"));
            return;
        }
    };

    if request.command.is_empty() {
        let body = serde_json::to_vec(&ExecResponse {
            exit_code: 2,
            stdout: String::new(),
            stderr: "command must not be empty".to_string(),
            timed_out: false,
        })
        .unwrap_or_default();
        let _ = stream.write_all(&body);
        return;
    }

    let mut command = Command::new(&request.command[0]);
    command.args(&request.command[1..]);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    begin_exec_registration(&exit_registry);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            cancel_exec_registration(&exit_registry);
            let body = serde_json::to_vec(&ExecResponse {
                exit_code: 127,
                stdout: String::new(),
                stderr: format!("failed to spawn '{}': {e}", request.command[0]),
                timed_out: false,
            })
            .unwrap_or_default();
            let _ = stream.write_all(&body);
            return;
        }
    };

    let pid = Pid::from_raw(child.id() as i32);
    let stdout_rx = spawn_reader(child.stdout.take().unwrap());
    let stderr_rx = spawn_reader(child.stderr.take().unwrap());
    let (exit_tx, exit_rx) = mpsc::channel();
    register_exec_waiter(&exit_registry, pid, exit_tx);

    let mut timed_out = false;
    let exit_code = if let Some(timeout_secs) = request.timeout_secs {
        match exit_rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
            Ok(code) => code,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                timed_out = true;
                let _ = child.kill();
                exit_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap_or(124)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => 1,
        }
    } else {
        exit_rx.recv().unwrap_or(1)
    };

    let stdout = stdout_rx.recv().unwrap_or_default();
    let stderr = stderr_rx.recv().unwrap_or_default();

    let body = serde_json::to_vec(&ExecResponse {
        exit_code,
        stdout,
        stderr,
        timed_out,
    })
    .unwrap_or_default();
    if let Err(e) = stream.write_all(&body) {
        log(format!("sandbox exec write failed: {e}"));
    }
}

/// See [`ControlRequest::Forked`].
fn resume_as_fork(entropy_hex: &str, unix_time_ns: u64) -> Result<(), String> {
    let entropy = decode_hex(entropy_hex).ok_or("the entropy is not hex")?;
    add_entropy_and_reseed(&entropy).map_err(|e| format!("reseeding the random generator: {e}"))?;
    set_realtime_clock(unix_time_ns).map_err(|e| format!("setting the clock: {e}"))?;
    Ok(())
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// Credit `entropy` to the kernel's input pool, then reseed its CRNG from
/// the pool now rather than at its next scheduled reseed, which could be a
/// minute away.
fn add_entropy_and_reseed(entropy: &[u8]) -> std::io::Result<()> {
    // From <linux/random.h>: _IOW('R', 0x03, int[2]) and _IO('R', 0x07).
    const RNDADDENTROPY: libc::Ioctl = 0x4008_5203;
    const RNDRESEEDCRNG: libc::Ioctl = 0x5207;
    const MAX_BYTES: usize = 256;
    /// `struct rand_pool_info` with room for `MAX_BYTES`.
    #[repr(C)]
    struct RandPoolInfo {
        entropy_count: libc::c_int,
        buf_size: libc::c_int,
        buf: [u8; MAX_BYTES],
    }

    if entropy.len() > MAX_BYTES {
        return Err(std::io::Error::other(format!(
            "{} bytes of entropy, more than {MAX_BYTES}",
            entropy.len()
        )));
    }
    let mut info = RandPoolInfo {
        entropy_count: (entropy.len() * 8) as libc::c_int,
        buf_size: entropy.len() as libc::c_int,
        buf: [0; MAX_BYTES],
    };
    info.buf[..entropy.len()].copy_from_slice(entropy);

    let urandom = std::fs::File::open("/dev/urandom")?;
    // SAFETY: `info` is a valid rand_pool_info whose buf holds buf_size bytes,
    // and both ioctls only read their argument.
    if unsafe { libc::ioctl(urandom.as_raw_fd(), RNDADDENTROPY, &info) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::ioctl(urandom.as_raw_fd(), RNDRESEEDCRNG) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn set_realtime_clock(unix_time_ns: u64) -> std::io::Result<()> {
    // SAFETY: an all-zero timespec is valid; the fields are set below.
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    time.tv_sec = (unix_time_ns / 1_000_000_000) as _;
    time.tv_nsec = (unix_time_ns % 1_000_000_000) as _;
    // SAFETY: `time` is a valid timespec.
    if unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &time) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Give each of `nics` its fork's MAC and address in place of its source's.
fn readdress(nics: &[NicAddress]) -> Result<(), String> {
    // Before any interface goes down, which drops the routes through it.
    let routes = std::fs::read_to_string("/proc/net/route")
        .map_err(|e| format!("reading the routes: {e}"))?;
    let socket = socket(
        AddressFamily::Inet,
        SockType::Datagram,
        SockFlag::SOCK_CLOEXEC,
        None,
    )
    .map_err(|e| format!("socket: {e}"))?;
    for nic in nics {
        let name = interface_with_mac(&nic.from)?;
        let mac = parse_mac(&nic.mac).ok_or_else(|| format!("{:?} is not a MAC", nic.mac))?;
        let gateways = default_gateways(&routes, &name);
        readdress_interface(socket.as_raw_fd(), &name, mac, nic, &gateways)
            .map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

fn interface_with_mac(mac: &str) -> Result<String, String> {
    let interfaces = std::fs::read_dir("/sys/class/net")
        .map_err(|e| format!("listing the network interfaces: {e}"))?;
    interfaces
        .flatten()
        .find(|interface| {
            std::fs::read_to_string(interface.path().join("address"))
                .is_ok_and(|address| address.trim().eq_ignore_ascii_case(mac))
        })
        .map(|interface| interface.file_name().to_string_lossy().into_owned())
        .ok_or_else(|| format!("no network interface has MAC {mac}"))
}

fn parse_mac(mac: &str) -> Option<[u8; 6]> {
    let bytes: Vec<u8> = mac
        .split(':')
        .map(|byte| u8::from_str_radix(byte, 16).ok())
        .collect::<Option<_>>()?;
    bytes.try_into().ok()
}

/// The gateways of `interface`'s default routes in `routes`, the text of
/// /proc/net/route.
fn default_gateways(routes: &str, interface: &str) -> Vec<Ipv4Addr> {
    routes
        .lines()
        .skip(1)
        .filter_map(|line| {
            // Iface Destination Gateway Flags RefCnt Use Metric Mask ...
            let fields: Vec<&str> = line.split_whitespace().collect();
            let [iface, destination, gateway, flags, _, _, _, mask, ..] = fields.as_slice() else {
                return None;
            };
            let flags = u16::from_str_radix(flags, 16).ok()?;
            let default = *iface == interface
                && *destination == "00000000"
                && *mask == "00000000"
                && flags & libc::RTF_GATEWAY != 0;
            // The address as the kernel holds it, printed as a native integer.
            let gateway = u32::from_str_radix(gateway, 16).ok()?;
            default.then(|| Ipv4Addr::from(gateway.to_ne_bytes()))
        })
        .collect()
}

/// Take `name` down, give it `nic`'s MAC and address, bring it up, and
/// route the default traffic through `gateways` again.
fn readdress_interface(
    fd: RawFd,
    name: &str,
    mac: [u8; 6],
    nic: &NicAddress,
    gateways: &[Ipv4Addr],
) -> std::io::Result<()> {
    /// The kernel's `struct rtentry` (<linux/route.h>), which only some
    /// targets' libc has.
    #[repr(C)]
    struct RtEntry {
        rt_pad1: libc::c_ulong,
        rt_dst: libc::sockaddr,
        rt_gateway: libc::sockaddr,
        rt_genmask: libc::sockaddr,
        rt_flags: libc::c_ushort,
        rt_pad2: libc::c_short,
        rt_pad3: libc::c_ulong,
        rt_pad4: *mut libc::c_void,
        rt_metric: libc::c_short,
        rt_dev: *mut libc::c_char,
        rt_mtu: libc::c_ulong,
        rt_window: libc::c_ulong,
        rt_irtt: libc::c_ushort,
    }

    let c_name = CString::new(name).map_err(std::io::Error::other)?;
    if c_name.as_bytes_with_nul().len() > libc::IFNAMSIZ {
        return Err(std::io::Error::other("interface name too long"));
    }
    // SAFETY: an all-zero ifreq is valid; the name is set below.
    let mut ifreq: libc::ifreq = unsafe { std::mem::zeroed() };
    for (dst, src) in ifreq.ifr_name.iter_mut().zip(c_name.as_bytes()) {
        *dst = *src as libc::c_char;
    }

    ioctl(fd, libc::SIOCGIFFLAGS as libc::Ioctl, &mut ifreq)?;
    // SAFETY: SIOCGIFFLAGS filled in the flags.
    let flags = unsafe { ifreq.ifr_ifru.ifru_flags };
    // Down first: Firecracker's virtio-net can't change a running NIC's MAC.
    ifreq.ifr_ifru.ifru_flags = flags & !(libc::IFF_UP as libc::c_short);
    ioctl(fd, libc::SIOCSIFFLAGS as libc::Ioctl, &mut ifreq)?;

    // SAFETY: an all-zero sockaddr is valid; its family and bytes are set below.
    let mut hwaddr: libc::sockaddr = unsafe { std::mem::zeroed() };
    hwaddr.sa_family = libc::ARPHRD_ETHER;
    for (dst, src) in hwaddr.sa_data.iter_mut().zip(mac) {
        *dst = src as libc::c_char;
    }
    ifreq.ifr_ifru.ifru_hwaddr = hwaddr;
    ioctl(fd, libc::SIOCSIFHWADDR as libc::Ioctl, &mut ifreq)?;
    ifreq.ifr_ifru.ifru_addr = sockaddr_of(nic.ip);
    ioctl(fd, libc::SIOCSIFADDR as libc::Ioctl, &mut ifreq)?;
    ifreq.ifr_ifru.ifru_netmask = sockaddr_of(nic.mask);
    ioctl(fd, libc::SIOCSIFNETMASK as libc::Ioctl, &mut ifreq)?;

    ifreq.ifr_ifru.ifru_flags = flags | libc::IFF_UP as libc::c_short;
    ioctl(fd, libc::SIOCSIFFLAGS as libc::Ioctl, &mut ifreq)?;

    for gateway in gateways {
        let mut route = RtEntry {
            rt_pad1: 0,
            rt_dst: sockaddr_of(Ipv4Addr::UNSPECIFIED),
            rt_gateway: sockaddr_of(*gateway),
            rt_genmask: sockaddr_of(Ipv4Addr::UNSPECIFIED),
            rt_flags: libc::RTF_UP | libc::RTF_GATEWAY,
            rt_pad2: 0,
            rt_pad3: 0,
            rt_pad4: std::ptr::null_mut(),
            rt_metric: 0,
            rt_dev: c_name.as_ptr() as *mut libc::c_char,
            rt_mtu: 0,
            rt_window: 0,
            rt_irtt: 0,
        };
        ioctl(fd, libc::SIOCADDRT as libc::Ioctl, &mut route)?;
    }
    Ok(())
}

fn sockaddr_of(ip: Ipv4Addr) -> libc::sockaddr {
    let addr = libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: 0,
        sin_addr: libc::in_addr {
            s_addr: u32::from_ne_bytes(ip.octets()),
        },
        sin_zero: [0; 8],
    };
    // SAFETY: both are 16 bytes: the socket API passes a sockaddr_in as a sockaddr.
    unsafe { std::mem::transmute::<libc::sockaddr_in, libc::sockaddr>(addr) }
}

/// An ioctl whose argument is `arg`.
fn ioctl<T>(fd: RawFd, request: libc::Ioctl, arg: &mut T) -> std::io::Result<()> {
    // SAFETY: each caller passes the struct its request reads or writes.
    if unsafe { libc::ioctl(fd, request, arg as *mut T) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn spawn_exec_server(exit_registry: ExitRegistry) {
    thread::spawn(move || {
        let listener_fd = match socket(
            AddressFamily::Vsock,
            SockType::Stream,
            SockFlag::empty(),
            None,
        ) {
            Ok(fd) => fd,
            Err(e) => {
                log(format!("sandbox exec vsock socket failed: {e}"));
                return;
            }
        };

        let addr = VsockAddr::new(libc::VMADDR_CID_ANY, SANDBOX_EXEC_PORT);
        if let Err(e) = bind(listener_fd.as_raw_fd(), &addr) {
            log(format!("sandbox exec vsock bind failed: {e}"));
            return;
        }
        if let Err(e) = listen(&listener_fd, Backlog::new(16).unwrap()) {
            log(format!("sandbox exec vsock listen failed: {e}"));
            return;
        }

        log(format!(
            "sandbox exec agent listening on vsock port {}",
            SANDBOX_EXEC_PORT
        ));

        loop {
            let accepted = match accept4(listener_fd.as_raw_fd(), SockFlag::SOCK_CLOEXEC) {
                Ok(fd) => fd,
                Err(e) => {
                    log(format!("sandbox exec accept failed: {e}"));
                    continue;
                }
            };

            let fd = unsafe { OwnedFd::from_raw_fd(accepted) };
            let registry = Arc::clone(&exit_registry);
            thread::spawn(move || serve_exec_connection(fd, registry));
        }
    });
}

/// Watch the input devices for the power button. qarax-node stops a Cloud
/// Hypervisor VM by pressing it (`vm.power-button`), and the kernel reports
/// the ACPI button as a KEY_POWER press.
fn spawn_power_button_watchers() {
    let devices = match std::fs::read_dir("/dev/input") {
        Ok(devices) => devices,
        Err(e) => {
            log(format!(
                "warning: no input devices ({e}), the power button won't shut the VM down"
            ));
            return;
        }
    };
    for device in devices.flatten() {
        if !device.file_name().to_string_lossy().starts_with("event") {
            continue;
        }
        match std::fs::File::open(device.path()) {
            Ok(file) => {
                thread::spawn(move || watch_power_button(file));
            }
            Err(e) => log(format!("warning: open {}: {e}", device.path().display())),
        }
    }
}

fn watch_power_button(mut device: std::fs::File) {
    let mut event = [0u8; std::mem::size_of::<libc::input_event>()];
    // evdev reads always return whole events.
    while device.read_exact(&mut event).is_ok() {
        // SAFETY: input_event is plain data, and `event` holds one in full.
        let event: libc::input_event = unsafe { std::ptr::read_unaligned(event.as_ptr().cast()) };
        if event.type_ == EV_KEY && event.code == KEY_POWER && event.value == 1 {
            shut_down("power button pressed", Ending::PowerOff);
        }
    }
}

/// Stop every process and power the VM off. Processes get SIGTERM and
/// SHUTDOWN_GRACE to exit before SIGKILL; the main loop powers off as soon
/// as they all have.
fn shut_down(reason: &str, ending: Ending) {
    // qarax-node presses the button again while it waits.
    if SHUTDOWN.set(ending).is_err() {
        return;
    }
    log(format!("{reason}, shutting down"));
    // -1 signals every process except init itself.
    let _ = kill(Pid::from_raw(-1), Signal::SIGTERM);
    thread::sleep(SHUTDOWN_GRACE);
    log(format!(
        "processes still running after {}s, killing them",
        SHUTDOWN_GRACE.as_secs()
    ));
    let _ = kill(Pid::from_raw(-1), Signal::SIGKILL);
    end_vm(ending);
}

fn end_vm(ending: Ending) -> ! {
    let (doing, command) = match ending {
        Ending::PowerOff => ("powering off", libc::RB_POWER_OFF),
        Ending::Reboot => ("rebooting", libc::RB_AUTOBOOT),
    };
    log(doing);
    unsafe {
        libc::sync();
        libc::reboot(command);
    }
    // reboot() only returns on failure. Exiting would panic the kernel; wait
    // for qarax-node to force the VM off instead.
    log(format!(
        "{doing} failed: {}",
        std::io::Error::last_os_error()
    ));
    loop {
        thread::park();
    }
}

fn main() {
    try_mount("proc", "/proc", "proc");
    try_mount("sysfs", "/sys", "sysfs");
    try_mount("devtmpfs", "/dev", "devtmpfs");

    let _ = std::os::unix::fs::symlink("/proc/self/fd", "/dev/fd");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/0", "/dev/stdin");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/1", "/dev/stdout");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/2", "/dev/stderr");

    maybe_switch_root();
    setup_loopback();

    let config: Config = std::fs::read_to_string("/.qarax-config.json")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    for var in &config.env {
        if let Some((key, val)) = var.split_once('=') {
            unsafe { std::env::set_var(key, val) };
        }
    }

    let mut argv = config.entrypoint;
    argv.extend(config.cmd);
    if argv.is_empty() {
        argv.push("/bin/sh".to_string());
    }

    log(format!("starting: {}", argv.join(" ")));

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            let err = Command::new(&argv[0]).args(&argv[1..]).exec();
            log(format!("failed to exec '{}': {}", argv[0], err));
            std::process::exit(1);
        }
        Ok(ForkResult::Parent { child }) => {
            let exit_registry: ExitRegistry = Arc::new(Mutex::new(ExitRegistryState::default()));
            spawn_exec_server(Arc::clone(&exit_registry));
            spawn_power_button_watchers();

            loop {
                match waitpid(Pid::from_raw(-1), None) {
                    Ok(status) => {
                        let Some(pid) = status.pid() else {
                            continue;
                        };

                        let exit_code = match status {
                            WaitStatus::Exited(_, code) => code,
                            WaitStatus::Signaled(_, sig, _) => 128 + sig as i32,
                            _ => continue,
                        };

                        if pid == child && SHUTDOWN.get().is_none() {
                            std::process::exit(exit_code);
                        }

                        record_child_exit(&exit_registry, pid, exit_code);
                    }
                    // Once shutting down, everything having exited means no
                    // need to wait out the rest of the grace period.
                    Err(nix::errno::Errno::ECHILD) => match SHUTDOWN.get() {
                        Some(&ending) => end_vm(ending),
                        None => std::process::exit(0),
                    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fork_notice_is_a_control_request() {
        let line = r#"{"forked":{"entropy":"00ff10","unix_time_ns":1763568000000000000}}"#;
        let Ok(ControlRequest::Forked {
            entropy,
            unix_time_ns,
        }) = serde_json::from_str(line)
        else {
            panic!("not a fork notice");
        };
        assert_eq!(decode_hex(&entropy), Some(vec![0x00, 0xff, 0x10]));
        assert_eq!(unix_time_ns, 1_763_568_000_000_000_000);
    }

    #[test]
    fn a_forks_addresses_are_a_control_request() {
        let line = r#"{"readdress":[{"from":"02:00:00:00:00:01","mac":"02:00:00:00:00:02","ip":"10.97.0.3","mask":"255.255.255.0"}]}"#;
        let Ok(ControlRequest::Readdress(nics)) = serde_json::from_str(line) else {
            panic!("not a fork's addresses");
        };
        let [nic] = nics.as_slice() else {
            panic!("{} NICs", nics.len());
        };
        assert_eq!(nic.from, "02:00:00:00:00:01");
        assert_eq!(parse_mac(&nic.mac), Some([2, 0, 0, 0, 0, 2]));
        assert_eq!(
            (nic.ip, nic.mask),
            (Ipv4Addr::new(10, 97, 0, 3), Ipv4Addr::new(255, 255, 255, 0))
        );
    }

    #[test]
    fn an_interfaces_default_routes_are_kept_by_their_gateways() {
        let hex = |ip: Ipv4Addr| format!("{:08X}", u32::from_ne_bytes(ip.octets()));
        let gateway = Ipv4Addr::new(10, 97, 0, 1);
        let subnet = Ipv4Addr::new(10, 97, 0, 0);
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        let any = Ipv4Addr::UNSPECIFIED;
        let routes = [
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT"
                .to_string(),
            format!(
                "eth0\t{}\t{}\t0003\t0\t0\t0\t{}\t0\t0\t0",
                hex(any),
                hex(gateway),
                hex(any)
            ),
            format!(
                "eth0\t{}\t{}\t0001\t0\t0\t0\t{}\t0\t0\t0",
                hex(subnet),
                hex(any),
                hex(mask)
            ),
            format!(
                "eth1\t{}\t{}\t0003\t0\t0\t0\t{}\t0\t0\t0",
                hex(any),
                hex(Ipv4Addr::new(10, 98, 0, 1)),
                hex(any)
            ),
        ]
        .join("\n");
        assert_eq!(default_gateways(&routes, "eth0"), vec![gateway]);
        assert_eq!(default_gateways(&routes, "eth2"), Vec::<Ipv4Addr>::new());
    }

    #[test]
    fn a_ping_is_a_control_request() {
        assert!(matches!(
            serde_json::from_str(r#"{"ping":{}}"#),
            Ok(ControlRequest::Ping {})
        ));
    }

    #[test]
    fn entropy_that_is_not_hex_is_refused() {
        for bad in ["abc", "zz", "+f"] {
            assert_eq!(decode_hex(bad), None, "{bad:?}");
        }
    }
}
