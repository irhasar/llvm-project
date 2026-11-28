use std::env;
use std::fs;
use std::hint::black_box;
use std::process::exit;
use std::time::Instant;

use llvm_rust_bitcode::{read_bitcode, write_bitcode};
use llvm_rust_ir::{parse_module, print_module, verify_module};
use llvm_rust_support::{Error, Result};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        exit(1);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    ParseAndPrint,
    ParseOnly,
    PrintOnly,
    Assemble,
    BitcodeWrite,
    BitcodeRead,
    Disassemble,
    Verify,
    AssembleVerify,
}

fn run() -> Result<()> {
    let mut iterations = 1000usize;
    let mut path = None;
    let mut mode = Mode::ParseAndPrint;
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
            "--parse-only" => mode = Mode::ParseOnly,
            "--print-only" => mode = Mode::PrintOnly,
            "--assemble" => mode = Mode::Assemble,
            "--bitcode-write" => mode = Mode::BitcodeWrite,
            "--bitcode-read" => mode = Mode::BitcodeRead,
            "--disassemble" => mode = Mode::Disassemble,
            "--verify" => mode = Mode::Verify,
            "--assemble-verify" => mode = Mode::AssembleVerify,
            other if other.starts_with('-') => {
                return Err(Error::io(format!("unknown option `{other}`")));
            }
            other => path = Some(other.to_string()),
        }
    }
    let path = path.ok_or_else(|| Error::io("missing input file"))?;
    let source = fs::read_to_string(&path)?;
    let mut parsed = if matches!(
        mode,
        Mode::PrintOnly | Mode::BitcodeWrite | Mode::BitcodeRead | Mode::Disassemble
    ) {
        Some(parse_module(&source)?)
    } else {
        None
    };
    let bitcode = match mode {
        Mode::BitcodeRead | Mode::Disassemble => {
            Some(write_bitcode(parsed.as_mut().expect("parsed module"))?)
        }
        _ => None,
    };

    let start = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..iterations {
        match mode {
            Mode::ParseOnly => {
                let module = parse_module(black_box(&source))?;
                bytes = 0;
                black_box(module);
            }
            Mode::PrintOnly => {
                let text = print_module(parsed.as_ref().expect("parsed module"));
                bytes = text.len();
                black_box(text);
            }
            Mode::ParseAndPrint => {
                let module = parse_module(black_box(&source))?;
                let text = print_module(&module);
                bytes = text.len();
                black_box(text);
            }
            Mode::Assemble => {
                let mut module = parse_module(black_box(&source))?;
                let bitcode = write_bitcode(&mut module)?;
                bytes = bitcode.len();
                black_box(bitcode);
            }
            Mode::AssembleVerify => {
                let mut module = parse_module(black_box(&source))?;
                let errors = verify_module(&module).unwrap_or_default();
                black_box(errors);
                let bitcode = write_bitcode(&mut module)?;
                bytes = bitcode.len();
                black_box(bitcode);
            }
            Mode::BitcodeWrite => {
                let bitcode = write_bitcode(parsed.as_mut().expect("parsed module"))?;
                bytes = bitcode.len();
                black_box(bitcode);
            }
            Mode::BitcodeRead => {
                let module = read_bitcode(bitcode.as_deref().expect("bitcode"))?;
                bytes = 0;
                black_box(module);
            }
            Mode::Disassemble => {
                let module = read_bitcode(bitcode.as_deref().expect("bitcode"))?;
                let text = print_module(&module);
                bytes = text.len();
                black_box(text);
            }
            Mode::Verify => {
                let module = parse_module(black_box(&source))?;
                bytes = match verify_module(&module) {
                    Ok(()) => 0,
                    Err(errors) => errors.len(),
                };
                black_box(bytes);
            }
        }
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
