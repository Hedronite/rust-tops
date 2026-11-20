//! Asking a tool for its version: one command, a time limit, and a reason in
//! words when it does not answer.
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long one `--version` ask may take.
pub(crate) const PROBE_LIMIT: Duration = Duration::from_secs(10);

/// How often a running ask is looked at.
const POLL: Duration = Duration::from_millis(5);

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
    wait_within(&mut child, deadline, limit)?;
    // The command has exited; what it left holding the pipe has until the
    // deadline to let go of it.
    output
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| give_up(&mut child, limit))
}

/// Waits for `child` to exit 0, or kills it when `deadline` has passed.
fn wait_within(child: &mut Child, deadline: Instant, limit: Duration) -> Result<(), String> {
    loop {
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
#[cfg(unix)]
fn kill_group(child: &Child) {
    use rustix::process::{kill_process_group, Pid, Signal};
    let leader = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
    if let Some(leader) = leader {
        kill_process_group(leader, Signal::KILL).ok();
    }
}

#[cfg(not(unix))]
fn kill_group(_child: &Child) {}

fn exit_words(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "was stopped by a signal".to_string(),
        |code| format!("exited {code}"),
    )
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

    /// A scratch directory of its own, removed when it goes out of scope.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
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

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
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
        assert!(
            gone(pid.trim()),
            "the child {} outlived the probe",
            pid.trim()
        );
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
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "{:?}",
            began.elapsed()
        );
        let pid = std::fs::read_to_string(scratch.0.join("child.pid")).expect("pid file");
        assert!(
            gone(pid.trim()),
            "the child {} outlived the probe",
            pid.trim()
        );
    }

    #[test]
    fn a_command_that_exits_having_started_nothing_is_not_held_to_the_deadline() {
        let began = Instant::now();
        let out = run_probe(&here(), &["sh", "-c", "echo line"], Duration::from_secs(30));
        assert_eq!(out.as_deref(), Ok("line\n"));
        assert!(began.elapsed() < Duration::from_secs(10));
    }
}
