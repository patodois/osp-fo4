#![forbid(unsafe_code)]
//! Disassembly of one `.pex` entry chosen by its archive path:
//! `pex_dump MANIFEST scripts/<path>.pex` prints the text of
//! `fo4_pex::disassemble` for the winning entry of that path.
use fo4_core::Budget;
use fo4_pex::{disassemble, Pex};

mod support;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("pex_dump MANIFEST scripts/<path>.pex".into());
    }
    let (m, vfs) = support::open(&args[0])?;
    let b = Budget::new(m.limits.max_memory);
    let bytes = vfs.read(&args[1], &b)?;
    let p = Pex::read(&bytes, &args[1], &b)?;
    print!("{}", disassemble(&p));
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
