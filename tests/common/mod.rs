//! Shared zero-dependency test infrastructure.
//!
//! Each integration-test crate pulls this in with `mod common;`. Cargo does
//! not compile `tests/common/` as a test target because it has no top-level
//! `common.rs` in `tests/` — the `common/mod.rs` layout is the standard way
//! to share helpers without dev-dependencies.
#![allow(dead_code)]

pub mod gen;
pub mod harness;
pub mod oracle;
pub mod prng;
