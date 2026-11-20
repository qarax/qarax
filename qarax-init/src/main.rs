use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
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
