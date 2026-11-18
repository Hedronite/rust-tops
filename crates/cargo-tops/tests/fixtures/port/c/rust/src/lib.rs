//! Port of the line counter.

/// Number of lines; a last line without a newline counts.
pub fn count_lines(s: &str) -> usize {
    s.lines().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_lines() {
        assert_eq!(count_lines(""), 0);
    }

    #[test]
    fn test_trailing_newline() {
        assert_eq!(count_lines("a\nb\n"), 2);
    }

    #[test]
    fn lines_unterminated_last_line() {
        assert_eq!(count_lines("a\nb"), 2);
    }
}
