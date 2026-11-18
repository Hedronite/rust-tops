//! Port of the word counter.

/// Number of whitespace-separated words.
pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// First word of the greatest length, or None.
pub fn longest_word(text: &str) -> Option<&str> {
    let mut best: Option<&str> = None;
    for word in text.split_whitespace() {
        if best.is_none_or(|b| word.len() > b.len()) {
            best = Some(word);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert_eq!(count_words(""), 0);
        assert_eq!(longest_word(""), None);
    }

    #[test]
    fn test_counts_words() {
        assert_eq!(count_words("a  b\tc\n"), 3);
    }

    #[test]
    fn test_first_of_a_tie_wins() {
        assert_eq!(longest_word("ab cd e"), Some("ab"));
    }
}
