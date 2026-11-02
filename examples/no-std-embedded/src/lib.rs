//! Fixed-capacity ring buffer for `#![no_std]` targets.
//!
//! Contract:
//! - Capacity is a compile-time power of two.
//! - `push` overwrites the oldest element when full if `overwrite` is true;
//!   otherwise it returns `Err(Overflow)`.
//! - `pop` returns `None` when empty.
//! - Length never exceeds capacity.

#![cfg_attr(not(feature = "std"), no_std)]
#![expect(
    clippy::exhaustive_enums,
    reason = "this crate already violates the strict table"
)]
#![expect(
    clippy::missing_assert_message,
    reason = "this crate already violates the strict table"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "this crate already violates the strict table"
)]
#![expect(missing_docs, reason = "this crate already violates the strict table")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    Full,
}

#[derive(Debug)]
pub struct Ring<T, const N: usize> {
    buf: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T, const N: usize> Ring<T, N> {
    pub const fn cap() -> usize {
        N
    }

    pub fn new() -> Self {
        // N must be a power of two and non-zero for wrap to stay branch-light.
        debug_assert!(N > 0 && N.is_power_of_two());
        Self {
            buf: [(); N].map(|_| None),
            head: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.len == N
    }

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "already in this crate when the strict table was turned on"
    )]
    pub fn push(&mut self, item: T, overwrite: bool) -> Result<(), Overflow> {
        if self.len == N {
            if !overwrite {
                return Err(Overflow::Full);
            }
            let idx = self.head;
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let tops_slot_76 = &mut self.buf[idx];
            *tops_slot_76 = Some(item);
            self.head = (self.head + 1) & (N - 1);
            return Ok(());
        }
        let idx = (self.head + self.len) & (N - 1);
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_slot_85 = &mut self.buf[idx];
        *tops_slot_85 = Some(item);
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let idx = self.head;
        self.head = (self.head + 1) & (N - 1);
        self.len -= 1;
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        self.buf[idx].take()
    }
}

impl<T, const N: usize> Default for Ring<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_fifo() {
        let mut r: Ring<u8, 4> = Ring::new();
        r.push(1, false).unwrap();
        r.push(2, false).unwrap();
        assert_eq!(r.pop(), Some(1));
        assert_eq!(r.pop(), Some(2));
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn overflow_without_overwrite() {
        let mut r: Ring<u8, 2> = Ring::new();
        r.push(1, false).unwrap();
        r.push(2, false).unwrap();
        assert_eq!(r.push(3, false), Err(Overflow::Full));
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn overwrite_drops_oldest() {
        let mut r: Ring<u8, 2> = Ring::new();
        r.push(1, false).unwrap();
        r.push(2, false).unwrap();
        r.push(3, true).unwrap();
        assert_eq!(r.pop(), Some(2));
        assert_eq!(r.pop(), Some(3));
    }
}
