use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::exit;

use llvm_rust_bitcode::write_bitcode;
use llvm_rust_ir::{parse_module, verify_module};
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
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" => {
                output = Some(
                    args.next()
                        .ok_or_else(|| Error::io("missing argument to -o"))?,
                );
            }
            "-h" | "--help" => {
                println!("usage: llvm-as-rust [input.ll] [-o output.bc]");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(Error::io(format!("unknown option `{other}`")));
            }
            other => input = Some(other.to_string()),
        }
    }

    let (path, source) = match input {
        Some(path) => (path.clone(), fs::read_to_string(&path)?),
        None => {
            let mut source = String::new();
            io::stdin().read_to_string(&mut source)?;
            ("<stdin>".to_string(), source)
        }
    };

    let mut module =
        parse_module(&source).map_err(|error| Error::parse(error.render(&path, &source)))?;
    if let Err(errors) = verify_module(&module) {
        for error in &errors {
            eprintln!("{}: {}", path, error.message);
        }
        return Err(Error::verify("module failed verification"));
    }
    let bytes = write_bitcode(&mut module)?;

    match output.as_deref() {
        Some("-") | None => io::stdout().write_all(&bytes)?,
        Some(path) => fs::write(path, &bytes)?,
    }
    Ok(())
}
