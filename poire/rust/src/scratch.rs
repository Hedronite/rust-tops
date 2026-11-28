//! RS-27 allocation churn on a hot path.

use std::fmt::{self, Write as _};

/// Renders rows of numbers as CSV lines, reusing one buffer across calls.
#[derive(Debug, Default)]
pub struct LineWriter {
    buf: String,
}

impl LineWriter {
    /// Renders `row` as one comma-separated line ending in `\n`.
    ///
    /// The buffer is cleared, not dropped, so its capacity carries over: once a
    /// line of a given width has been rendered, rendering another as wide does
    /// not reallocate the buffer.
    ///
    /// # Errors
    /// None in practice: writing into a `String` cannot fail. The `Result` is
    /// the one `fmt::Write` declares.
    pub fn render(&mut self, row: &[u64]) -> Result<&str, fmt::Error> {
        self.buf.clear();
        for (i, n) in row.iter().enumerate() {
            if i > 0 {
                self.buf.push(',');
            }
            write!(self.buf, "{n}")?;
        }
        self.buf.push('\n');
        Ok(&self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_csv() {
        let mut w = LineWriter::default();
        assert_eq!(
            w.render(&[1, 22, 333]).unwrap(),
            "1,22,333\n",
            "three fields"
        );
        assert_eq!(
            w.render(&[]).unwrap(),
            "\n",
            "an empty row is an empty line"
        );
    }

    #[test]
    fn the_buffer_is_not_reallocated_after_warm_up() {
        let mut w = LineWriter::default();
        let row = [u64::MAX, 0, 42];
        let first = w.render(&row).unwrap().as_ptr();
        let capacity = w.buf.capacity();
        for _ in 0..1_000 {
            let again = w.render(&row).unwrap().as_ptr();
            assert_eq!(again, first, "same allocation");
        }
        assert_eq!(w.buf.capacity(), capacity, "capacity unchanged");
    }
}
