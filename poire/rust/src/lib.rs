//! Checked exemplars for Rust-POIRE 0.2.
//!
//! Each module holds the healthy form of one or more conditions from the
//! Rust-POIRE catalog (`RS-NN`). The [`counter`] module holds counter-examples.
//! Each counter-example that a lint can see carries `#[expect(lint, reason)]`,
//! so the build fails the day that lint stops firing on it.

pub mod arena;
pub mod cache;
pub mod counter;
pub mod frames;
pub mod ids;
pub mod layout;
pub mod manifest;
pub mod scratch;
pub mod tasks;
pub mod views;
pub mod webhook;
