//! RS-24 wasteful layout on long-lived or numerous data.

use std::sync::Arc;

/// An interned, shared name. One allocation holds the counts and the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Name(Arc<str>);

impl Name {
    /// Interns `text`.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self(Arc::from(text))
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A lookup table frozen after it is built: no spare capacity, one word smaller than a `Vec`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table(Box<[u32]>);

impl From<Vec<u32>> for Table {
    fn from(values: Vec<u32>) -> Self {
        Self(values.into_boxed_slice())
    }
}

impl Table {
    /// The value at `index`, if any.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<u32> {
        self.0.get(index).copied()
    }
}

/// A protocol event. The rare large variant is boxed, so every event is two words.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A heartbeat.
    Ping,
    /// A counter update.
    Count(u64),
    /// A rare diagnostic dump.
    Dump(Box<[u8; 1024]>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sizes_are_what_the_docs_say() {
        assert_eq!(
            size_of::<Event>(),
            16,
            "boxed dump keeps the enum at two words"
        );
        assert_eq!(size_of::<Table>(), 16, "pointer and length, no capacity");
        assert_eq!(size_of::<Vec<u32>>(), 24, "a Vec carries capacity too");
        // Arc<str> is a wide handle (16 bytes) but one allocation and one hop.
        // Arc<String> is a thin handle (8 bytes) but two allocations and two hops.
        assert_eq!(size_of::<Name>(), 16, "wide handle");
        assert_eq!(size_of::<Arc<String>>(), 8, "thin handle, extra hop");
    }

    #[test]
    fn values_read_back() {
        let table = Table::from(vec![3, 5]);
        assert_eq!(
            (table.get(1), table.get(2)),
            (Some(5), None),
            "in and out of range"
        );
        assert_eq!(Name::new("lapis").as_str(), "lapis", "name round-trips");
    }
}
