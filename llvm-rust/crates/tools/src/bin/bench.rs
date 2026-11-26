use std::env;
use std::fs;
use std::hint::black_box;
use std::process::exit;
use std::time::Instant;

use llvm_rust_ir::{parse_module, print_module};
use llvm_rust_support::{Error, Result};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        exit(1);
    }
}

fn run() -> Result<()> {
    let mut iterations = 1000usize;
    let mut path = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-n" | "--iterations" => {
                iterations = args
                    .next()
                    .ok_or_else(|| Error::io("missing iteration count"))?
                    .parse()
                    .map_err(|_| Error::io("invalid iteration count"))?;
            }
            other if other.starts_with('-') => {
                return Err(Error::io(format!("unknown option `{other}`")));
            }
            other => path = Some(other.to_string()),
        }
    }
    let path = path.ok_or_else(|| Error::io("missing input file"))?;
    let source = fs::read_to_string(&path)?;

    let start = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..iterations {
        let module = parse_module(black_box(&source))?;
        let text = print_module(&module);
        bytes = text.len();
        black_box(text);
    }
    let elapsed = start.elapsed();
    println!(
        "iterations={} total_ms={:.3} per_iter_us={:.3} output_bytes={}",
        iterations,
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64,
        bytes
    );
    Ok(())
}
