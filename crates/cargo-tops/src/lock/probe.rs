//! Asking a tool for its version: one command, a time limit, and a reason in
//! words when it does not answer.
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
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
/// `timed out after <limit>`.
pub(crate) fn run_probe(dir: &Path, argv: &[&str], limit: Duration) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("no command")?;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "could not run".to_string())?;
    let reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            out.read_to_end(&mut bytes).ok();
            String::from_utf8_lossy(&bytes).into_owned()
        })
    });
    wait_within(&mut child, limit)?;
    Ok(reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default())
}

/// Waits for `child` to exit 0, or kills it when `limit` has passed.
fn wait_within(child: &mut Child, limit: Duration) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    loop {
        let status = child
            .try_wait()
            .map_err(|_| "could not wait for it".to_string())?;
        match status {
            Some(status) => return finished(status),
            None if Instant::now() >= deadline => return give_up(child, limit),
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

/// Kills `child`, which has run past `limit`, and says so.
fn give_up(child: &mut Child, limit: Duration) -> Result<(), String> {
    child.kill().ok();
    child.wait().ok();
    Err(format!("timed out after {limit:?}"))
}

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
}
