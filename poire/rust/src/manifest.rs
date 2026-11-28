//! RS-25 monomorphization bloat: a generic shell over one non-generic body.

use std::io;
use std::path::Path;

/// Reads a manifest file into a frozen byte buffer.
///
/// Callers pass `&str`, `String`, `&Path` or `PathBuf`. Only the one-line shell
/// is generic, so each caller type adds a call, not a second copy of the body.
///
/// # Errors
/// Any error from reading the file.
pub fn load_manifest(path: impl AsRef<Path>) -> io::Result<Box<[u8]>> {
    fn load(path: &Path) -> io::Result<Box<[u8]>> {
        Ok(std::fs::read(path)?.into_boxed_slice())
    }
    load(path.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_was_written() {
        let path = std::env::temp_dir().join(format!("poire-manifest-{}", std::process::id()));
        std::fs::write(&path, b"name = \"poire\"").unwrap();
        let from_path = load_manifest(&path).unwrap();
        let from_string = load_manifest(path.to_string_lossy().into_owned()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(&*from_path, b"name = \"poire\"", "contents read back");
        assert_eq!(
            from_path, from_string,
            "both caller types read the same bytes"
        );
    }
}
