//! LLVM bitstream and bitcode support for the Track B Rust core.

#![forbid(unsafe_code)]

mod attrs;
pub mod bitstream;
pub mod reader;
pub mod writer;

pub use reader::{ReadOptions, read_bitcode, read_bitcode_with_options};
pub use writer::{WriteOptions, write_bitcode, write_bitcode_with_options};
