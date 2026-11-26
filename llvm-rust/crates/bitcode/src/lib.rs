//! LLVM bitstream and bitcode support for the Track B Rust core.

#![forbid(unsafe_code)]

pub mod bitstream;
pub mod reader;
pub mod writer;

pub use reader::read_bitcode;
pub use writer::write_bitcode;
