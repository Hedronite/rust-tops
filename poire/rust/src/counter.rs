//! Counter-examples. Do not copy these.
//!
//! Each item shows one condition from the Rust-POIRE catalog. Where a lint can
//! see the condition, the item carries `#[expect(lint, reason)]`: if the lint
//! stops firing, `unfulfilled_lint_expectations` fails the build under
//! `-D warnings`. That is the proof the detector the catalog names is live.
//! Conditions no lint can see are shown in the document. Where a test can show
//! the harm, that test sits beside the healthy form (see `frames`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// RS-02: panics on input instead of returning an error.
#[expect(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    reason = "counter-example RS-02; the build fails if these lints stop firing"
)]
#[must_use]
pub fn port_or_panic(text: &str) -> u16 {
    text.parse().unwrap()
}

/// RS-03: a boolean mode flag where an enum names the states.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "counter-example RS-03; the build fails if this lint stops firing"
)]
#[must_use]
pub fn render(text: &str, bold: bool) -> String {
    if bold {
        format!("**{text}**")
    } else {
        text.to_owned()
    }
}

/// RS-04: the guard lives across `.await`, so the future is not `Send` and
/// every other caller waits on the fetch.
#[expect(
    clippy::await_holding_lock,
    reason = "counter-example RS-04; the build fails if this lint stops firing"
)]
pub async fn held_across_await(
    map: &Mutex<HashMap<String, u64>>,
    key: &str,
    fetch: impl Future<Output = u64>,
) -> Option<u64> {
    let mut guard = map.lock().ok()?;
    let value = fetch.await;
    guard.insert(key.to_owned(), value);
    Some(value)
}

/// RS-12: a clone of a value that is never used again.
#[expect(
    clippy::redundant_clone,
    reason = "counter-example RS-12; the build fails if this lint stops firing"
)]
#[must_use]
pub fn label_len(id: u64) -> usize {
    let text = id.to_string();
    let copy = text.clone();
    copy.len()
}

/// RS-13: `&Vec<u8>` where `&[u8]` serves every caller.
#[expect(
    clippy::ptr_arg,
    reason = "counter-example RS-13; the build fails if this lint stops firing"
)]
#[must_use]
pub fn zero_bytes(data: &Vec<u8>) -> usize {
    data.iter().filter(|&&b| b == 0).count()
}

/// RS-13: takes ownership of a `String` it only reads.
#[expect(
    clippy::needless_pass_by_value,
    reason = "counter-example RS-13; the build fails if this lint stops firing"
)]
#[must_use]
pub fn shout_len(text: String) -> usize {
    text.len()
}

/// RS-14: a `match` that `let ... else` says in one line.
#[expect(
    clippy::manual_let_else,
    reason = "counter-example RS-14; the build fails if this lint stops firing"
)]
#[must_use]
pub fn first_word(text: &str) -> &str {
    let word = match text.split_whitespace().next() {
        Some(word) => word,
        None => return "",
    };
    word.trim_matches('.')
}

/// RS-19: an index loop where an iterator states the job.
#[expect(
    clippy::needless_range_loop,
    clippy::indexing_slicing,
    reason = "counter-example RS-19; the build fails if these lints stop firing"
)]
#[must_use]
pub fn sum(values: &[u64]) -> u64 {
    let mut total = 0;
    for i in 0..values.len() {
        total += values[i];
    }
    total
}

/// RS-22: collects a vector only to count it.
#[expect(
    clippy::needless_collect,
    reason = "counter-example RS-22; the build fails if this lint stops firing"
)]
#[must_use]
pub fn count_even(values: &[u32]) -> usize {
    values
        .iter()
        .filter(|n| *n % 2 == 0)
        .collect::<Vec<_>>()
        .len()
}

/// RS-24: one rare variant sizes every value of the enum.
#[expect(
    clippy::large_enum_variant,
    reason = "counter-example RS-24; the build fails if this lint stops firing"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FatEvent {
    /// A heartbeat, 1,032 bytes wide because of its sibling.
    Ping,
    /// A rare diagnostic dump, stored inline.
    Dump([u8; 1024]),
}

/// RS-24: a shared `String` behind `Arc`: two allocations and two hops per read.
#[expect(
    clippy::rc_buffer,
    reason = "counter-example RS-24; the build fails if this lint stops firing"
)]
#[derive(Debug, Clone)]
pub struct SharedName(Arc<String>);

impl SharedName {
    /// The name's length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the name is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// RS-27: a fresh `String` allocated by `format!` for every field.
#[expect(
    clippy::format_push_string,
    reason = "counter-example RS-27; the build fails if this lint stops firing"
)]
#[must_use]
pub fn render_fresh(row: &[u64]) -> String {
    let mut out = String::new();
    for n in row {
        out.push_str(&format!("{n},"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The counter-examples are unhealthy, not wrong. These tests pin what they
    // compute, so mutation testing judges them like any other code.
    #[test]
    fn they_compute_what_they_claim() {
        assert_eq!(port_or_panic("8080"), 8080, "parses");
        assert_eq!(render("a", true), "**a**", "bold");
        assert_eq!(render("a", false), "a", "plain");
        assert_eq!(label_len(12_345), 5, "five digits");
        assert_eq!(zero_bytes(&vec![0, 1, 0]), 2, "two zeros");
        assert_eq!(shout_len(String::from("abc")), 3, "three bytes");
        assert_eq!(first_word("hi. there"), "hi", "first word, dot trimmed");
        assert_eq!(first_word("  "), "", "no word");
        assert_eq!(sum(&[1, 2, 3]), 6, "sum");
        assert_eq!(count_even(&[1, 2, 4]), 2, "two evens, one odd");
        assert_eq!(render_fresh(&[1, 2]), "1,2,", "trailing comma");
        let name = SharedName(Arc::new(String::from("ab")));
        assert_eq!((name.len(), name.is_empty()), (2, false), "two bytes");
        assert!(SharedName(Arc::new(String::new())).is_empty(), "empty name");
    }

    #[tokio::test]
    async fn the_held_guard_still_stores() {
        let map = Mutex::new(HashMap::new());
        let got = held_across_await(&map, "k", async { 7 }).await;
        assert_eq!(got, Some(7), "returns the fetched value");
        assert_eq!(
            map.lock().unwrap().get("k").copied(),
            Some(7),
            "and stores it"
        );
    }
}
