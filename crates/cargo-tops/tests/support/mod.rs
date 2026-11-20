//! Helpers the gate tests share.
use std::fs;
use std::path::Path;

/// A `rustc` that prints the toolchain pin `init` writes into `rust-tops.lock`,
/// so a gate that asks the toolchain for its version never depends on which
/// compiler the machine running the test has.
pub(crate) fn stub_rustc(bin: &Path) {
    let path = bin.join("rustc");
    fs::write(&path, "#!/bin/sh\necho 'rustc 1.99.1 (stub 2026-01-01)'\n")
        .expect("write rustc stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod rustc stub");
    }
}
