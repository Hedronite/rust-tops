//! Asking a tool for its version: one command, a time limit, and a reason in
//! words when it does not answer.
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long one `--version` ask may take.
pub(crate) const PROBE_LIMIT: Duration = Duration::from_secs(10);

/// How often a running ask is looked at.
const POLL: Duration = Duration::from_millis(5);

#[cfg(unix)]
mod signal_forward {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock, PoisonError};

    use rustix::process::{kill_process_group, Pid, Signal};

    static SIGNAL_HANDLER: OnceLock<()> = OnceLock::new();
    static INTERRUPTED: AtomicBool = AtomicBool::new(false);
    static ACTIVE_PROBES: Mutex<Vec<u32>> = Mutex::new(Vec::new());

    /// Registers a one-time `SIGINT`/`SIGTERM` forwarder so terminating
    /// `cargo-tops` during a hung probe also kills the probe's process group.
    ///
    /// `PR_SET_PDEATHSIG` is intentionally deferred under `unsafe_code = "forbid"`
    /// (RT-P6): `CommandExt::pre_exec` and raw `prctl` both require `unsafe`.
    pub(super) fn ensure_handler() {
        SIGNAL_HANDLER.get_or_init(|| {
            let Ok(mut signals) = signal_hook::iterator::Signals::new([
                signal_hook::consts::signal::SIGINT,
                signal_hook::consts::signal::SIGTERM,
            ]) else {
                return;
            };
            std::thread::spawn(move || {
                for sig in signals.forever() {
                    INTERRUPTED.store(true, Ordering::SeqCst);
                    kill_groups(&ACTIVE_PROBES);
                    signal_hook::low_level::emulate_default_handler(sig).ok();
                }
            });
        });
    }

    pub(super) fn kill_pgid(pid: u32) {
        let leader = i32::try_from(pid).ok().and_then(Pid::from_raw);
        if let Some(leader) = leader {
            kill_process_group(leader, Signal::KILL).ok();
        }
    }

    pub(super) fn kill_groups(probes: &Mutex<Vec<u32>>) {
        let active = probes.lock().unwrap_or_else(PoisonError::into_inner);
        for &pid in &*active {
            kill_pgid(pid);
        }
    }

    pub(super) fn interrupted_flag() -> &'static AtomicBool {
        &INTERRUPTED
    }

    pub(super) fn active_probes() -> &'static Mutex<Vec<u32>> {
        &ACTIVE_PROBES
    }

    /// Tracks a live probe's process group while `run_probe` waits on it.
    pub(super) struct ProbeGuard<'a> {
        probes: &'a Mutex<Vec<u32>>,
        pid: u32,
    }

    impl<'a> ProbeGuard<'a> {
        pub(super) fn register(
            probes: &'a Mutex<Vec<u32>>,
            pid: u32,
            interrupted: &AtomicBool,
        ) -> Self {
            probes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(pid);
            if interrupted.load(Ordering::SeqCst) {
                kill_pgid(pid);
            }
            Self { probes, pid }
        }
    }

    impl Drop for ProbeGuard<'_> {
        fn drop(&mut self) {
            self.probes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|&id| id != self.pid);
        }
    }
}

#[cfg(not(unix))]
mod signal_forward {
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;
    static NEVER: AtomicBool = AtomicBool::new(false);
    static EMPTY: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    pub(super) fn ensure_handler() {}
    pub(super) fn interrupted_flag() -> &'static AtomicBool {
        &NEVER
    }
    pub(super) fn active_probes() -> &'static Mutex<Vec<u32>> {
        &EMPTY
    }
    pub(super) fn kill_pgid(_pid: u32) {}
    pub(super) struct ProbeGuard;
    impl ProbeGuard {
        pub(super) fn register(
            _probes: &'static Mutex<Vec<u32>>,
            _pid: u32,
            _interrupted: &AtomicBool,
        ) -> Self {
            Self
        }
    }
}

/// A probe that runs each argv in `dir` with the [`PROBE_LIMIT`].
pub(crate) fn probe_in(dir: &Path) -> impl FnMut(&[&str]) -> Result<String, String> + '_ {
    move |argv| run_probe(dir, argv, PROBE_LIMIT)
}

/// What `argv` printed on stdout when it exited 0 within `limit`; otherwise
/// the reason, as `could not run`, `exited N`, `was stopped by a signal` or
/// `timed out after <limit>`. The limit covers the whole ask, the read of its
/// output included. The command runs in a group of its own, and a command that
/// runs past the limit is killed with everything it started.
pub(crate) fn run_probe(dir: &Path, argv: &[&str], limit: Duration) -> Result<String, String> {
    signal_forward::ensure_handler();
    run_probe_with(dir, argv, limit, signal_forward::interrupted_flag())
}

fn run_probe_with(
    dir: &Path,
    argv: &[&str],
    limit: Duration,
    interrupted: &AtomicBool,
) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("no command")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().map_err(|_| "could not run".to_string())?;
    let _guard = signal_forward::ProbeGuard::register(
        signal_forward::active_probes(),
        child.id(),
        interrupted,
    );
    let (sender, output) = mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            out.read_to_end(&mut bytes).ok();
            sender
                .send(String::from_utf8_lossy(&bytes).into_owned())
                .ok();
        });
    }
    let deadline = Instant::now() + limit;
    wait_within(&mut child, deadline, limit, interrupted)?;
    // The command has exited; what it left holding the pipe has until the
    // deadline to let go of it.
    output
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| stop_or_timeout(&mut child, limit, interrupted))
}

/// Waits for `child` to exit 0, or kills it when `deadline` has passed or
/// an interrupt signal arrived.
fn wait_within(
    child: &mut Child,
    deadline: Instant,
    limit: Duration,
    interrupted: &AtomicBool,
) -> Result<(), String> {
    loop {
        if interrupted.load(Ordering::SeqCst) {
            return Err(wait_for_signal_exit(child));
        }
        let status = child
            .try_wait()
            .map_err(|_| "could not wait for it".to_string())?;
        match status {
            Some(status) => return finished(status),
            None if Instant::now() >= deadline => return Err(give_up(child, limit)),
            None => std::thread::sleep(POLL),
        }
    }
}

fn stop_or_timeout(child: &mut Child, limit: Duration, interrupted: &AtomicBool) -> String {
    if interrupted.load(Ordering::SeqCst) {
        wait_for_signal_exit(child)
    } else {
        give_up(child, limit)
    }
}

/// Gives the signal thread time to terminate `cargo-tops` with the forwarded
/// signal before returning an error, preventing a race to `exit(3)`.
fn wait_for_signal_exit(child: &mut Child) -> String {
    child.wait().ok();
    std::thread::sleep(Duration::from_millis(200));
    "was stopped by a signal".to_string()
}

fn finished(status: ExitStatus) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(exit_words(status))
    }
}

/// Kills `child`, which has run past `limit`, with the group it leads, and says
/// so.
fn give_up(child: &mut Child, limit: Duration) -> String {
    kill_group(child);
    child.kill().ok();
    child.wait().ok();
    format!("timed out after {limit:?}")
}

/// Ends the process group `child` leads, so a hung tool does not leave its own
/// children behind.
fn kill_group(child: &Child) {
    signal_forward::kill_pgid(child.id());
}

fn exit_words(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "was stopped by a signal".to_string(),
        |code| format!("exited {code}"),
    )
}

#[cfg(test)]
pub(super) struct Scratch(pub(super) std::path::PathBuf);

#[cfg(test)]
impl Scratch {
    pub(super) fn new(name: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "cargo-tops-probe-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        Self(dir)
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here() -> std::path::PathBuf {
        std::env::current_dir().expect("cwd")
    }

    #[test]
    fn a_command_that_exits_0_gives_its_stdout() {
        let out = run_probe(&here(), &["sh", "-c", "echo hi; echo err >&2"], PROBE_LIMIT);
        assert_eq!(out.as_deref(), Ok("hi\n"));
    }

    #[test]
    fn a_command_that_exits_non_zero_says_which_code() {
        let out = run_probe(&here(), &["sh", "-c", "echo hi; exit 101"], PROBE_LIMIT);
        assert_eq!(out, Err("exited 101".to_string()));
    }

    #[test]
    fn a_command_that_does_not_exist_could_not_run() {
        let out = run_probe(&here(), &["a-tool-nobody-has-installed"], PROBE_LIMIT);
        assert_eq!(out, Err("could not run".to_string()));
        assert_eq!(
            run_probe(&here(), &[], PROBE_LIMIT),
            Err("no command".to_string())
        );
    }

    #[test]
    fn a_command_that_runs_past_the_limit_is_killed_and_says_so() {
        let began = Instant::now();
        let out = run_probe(
            &here(),
            &["sh", "-c", "sleep 30"],
            Duration::from_millis(200),
        );
        assert_eq!(out, Err("timed out after 200ms".to_string()));
        assert!(began.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_command_killed_by_a_signal_says_so() {
        let out = run_probe(&here(), &["sh", "-c", "kill -9 $$"], PROBE_LIMIT);
        assert_eq!(out, Err("was stopped by a signal".to_string()));
    }

    #[test]
    fn the_limit_is_ten_seconds() {
        assert_eq!(PROBE_LIMIT, Duration::from_secs(10));
    }

    #[test]
    fn the_probe_runs_in_the_directory_it_is_given() {
        let dir = std::env::temp_dir();
        let out = probe_in(&dir)(&["pwd"]).expect("pwd");
        assert_eq!(
            std::path::Path::new(out.trim()).canonicalize().ok(),
            dir.canonicalize().ok()
        );
    }

    /// Whether the process `pid` is still there, waiting up to two seconds for
    /// a killed one to be reaped.
    fn gone(pid: &str) -> bool {
        for _ in 0..40 {
            let alive = Command::new("kill")
                .args(["-0", pid])
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !alive {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn a_hung_command_is_killed_with_the_children_it_started() {
        let scratch = Scratch::new("tree");
        let script = "sleep 60 & echo $! > child.pid; sleep 60";
        let out = run_probe(
            &scratch.0,
            &["sh", "-c", script],
            Duration::from_millis(500),
        );
        assert_eq!(out, Err("timed out after 500ms".to_string()));
        let pid = std::fs::read_to_string(scratch.0.join("child.pid")).expect("pid file");
        assert!(gone(pid.trim()), "child outlived probe");
    }

    #[test]
    fn a_command_that_leaves_a_child_holding_its_output_is_bounded_and_the_child_ends() {
        let scratch = Scratch::new("holder");
        let script = "echo line; sleep 60 & echo $! > child.pid";
        let began = Instant::now();
        let out = run_probe(
            &scratch.0,
            &["sh", "-c", script],
            Duration::from_millis(500),
        );
        assert_eq!(out, Err("timed out after 500ms".to_string()));
        assert!(began.elapsed() < Duration::from_secs(10));
        let pid = std::fs::read_to_string(scratch.0.join("child.pid")).expect("pid file");
        assert!(gone(pid.trim()), "child outlived probe");
    }

    #[test]
    fn a_command_that_exits_having_started_nothing_is_not_held_to_the_deadline() {
        let began = Instant::now();
        let out = run_probe(&here(), &["sh", "-c", "echo line"], Duration::from_secs(30));
        assert_eq!(out.as_deref(), Ok("line\n"));
        assert!(began.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn active_probes_guard_clears_and_kills_live_groups() {
        let probes = std::sync::Mutex::new(Vec::new());
        let flag = AtomicBool::new(false);
        let sentinel_pid = u32::MAX;
        {
            let _guard = signal_forward::ProbeGuard::register(&probes, sentinel_pid, &flag);
            assert!(probes.lock().expect("lock").contains(&sentinel_pid));
        }
        assert!(probes.lock().expect("lock").is_empty());

        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 60"]);
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().expect("spawn");
        {
            let _guard = signal_forward::ProbeGuard::register(&probes, child.id(), &flag);
            signal_forward::kill_groups(&probes);
        }
        assert!(!child.wait().expect("wait").success());
    }

    #[cfg(unix)]
    #[test]
    fn interrupted_flag_stops_wait_and_timeout_helpers() {
        let probes = std::sync::Mutex::new(Vec::new());
        let flag = AtomicBool::new(false);
        let interrupted = AtomicBool::new(true);
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 60"]);
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().expect("spawn");
        let _guard = signal_forward::ProbeGuard::register(&probes, child.id(), &interrupted);
        assert!(!child.wait().expect("wait").success());

        let out = run_probe_with(
            &here(),
            &["sh", "-c", "sleep 30"],
            Duration::from_secs(5),
            &interrupted,
        );
        assert_eq!(out, Err("was stopped by a signal".to_string()));

        let mut quick = Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("quick");
        assert_eq!(
            stop_or_timeout(&mut quick, Duration::from_millis(50), &interrupted),
            "was stopped by a signal"
        );
        let mut slow_cmd = Command::new("sh");
        slow_cmd.args(["-c", "sleep 30"]);
        std::os::unix::process::CommandExt::process_group(&mut slow_cmd, 0);
        let mut slow = slow_cmd.spawn().expect("slow");
        assert_eq!(
            stop_or_timeout(&mut slow, Duration::from_millis(50), &flag),
            "timed out after 50ms"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_detached_setsid_grandchild_holding_stdout_is_bounded_by_deadline() {
        let scratch = Scratch::new("setsid");
        let pid_file = scratch.0.join("detached.pid");
        let grandchild_py = format!(
            "import os, pathlib, time; os.setsid(); pathlib.Path({:?}).write_text(str(os.getpid())); print('detached', flush=True); time.sleep(60)",
            pid_file.to_string_lossy()
        );
        let parent_py = format!(
            "import subprocess, sys; subprocess.Popen([sys.executable, '-c', {:?}])",
            grandchild_py
        );
        let began = Instant::now();
        let out = run_probe(
            &scratch.0,
            &["python3", "-c", &parent_py],
            Duration::from_millis(400),
        );
        let elapsed = began.elapsed();
        let pid = std::fs::read_to_string(&pid_file).expect("detached pid");
        let trimmed = pid.trim();
        Command::new("kill").args(["-9", trimmed]).status().ok();
        assert!(gone(trimmed), "detached process failed to exit");
        assert_eq!(out, Err("timed out after 400ms".to_string()));
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    }
}
