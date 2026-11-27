use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::exit;

use llvm_rust_bitcode::{ReadOptions, read_bitcode_with_options};
use llvm_rust_ir::print_module;
use llvm_rust_support::{Error, Result};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        exit(1);
    }
}

fn run() -> Result<()> {
    let mut input = None;
    let mut output = None;
    let mut llvm19 = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" => {
                output = Some(
                    args.next()
                        .ok_or_else(|| Error::io("missing argument to -o"))?,
                );
            }
            "--llvm19" => llvm19 = true,
            "-h" | "--help" => {
                println!("usage: llvm-dis-rust [--llvm19] [input.bc] [-o output.ll]");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(Error::io(format!("unknown option `{other}`")));
            }
            other => input = Some(other.to_string()),
        }
    }

    let bytes = match input {
        Some(path) => fs::read(&path)?,
        None => {
            let mut bytes = Vec::new();
            io::stdin().read_to_end(&mut bytes)?;
            bytes
        }
    };
    let module = read_bitcode_with_options(
        &bytes,
        ReadOptions {
            llvm19_compat: llvm19,
        },
    )?;
    let text = print_module(&module);
    match output.as_deref() {
        Some("-") | None => io::stdout().write_all(text.as_bytes())?,
        Some(path) => fs::write(path, text.as_bytes())?,
    }
    io::stdout().flush()?;
    // The process is exiting; let the kernel reclaim the module and text
    // allocations instead of walking the arena and every owned string.
    std::process::exit(0);
}
