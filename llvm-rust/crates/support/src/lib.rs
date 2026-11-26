//! Shared primitives for the Rust-native LLVM core.
//!
//! Track B uses arena plus index handles instead of intrusive pointers, and
//! `Result` instead of aborts. This crate has no unsafe code.

#![forbid(unsafe_code)]

pub mod arena;
pub mod diag;
pub mod error;
pub mod symbol;

pub use arena::{Arena, Idx};
pub use diag::{Diagnostic, Diagnostics, Severity, Span};
pub use error::{Error, ErrorKind, Result};
pub use symbol::{Interner, Symbol};
