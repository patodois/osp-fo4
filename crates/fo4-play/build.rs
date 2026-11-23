//! Compiles `quest_run`'s source (an example of `fo4-vm`) into this crate
//! so the play mode runs the same new-game code: the copy in `OUT_DIR`
//! drops the file's inner attributes and module docs, takes its arguments
//! from the caller (`run_with`) and calls `play::play_loop` where it calls
//! its clock loop. `play_loop` runs the clock loop unchanged when no
//! driver is installed. Each patch must match exactly once, or the build
//! stops naming it: the request for a stepping entry point in `fo4-vm`
//! itself is in `spec/play.md`.
use std::path::PathBuf;

const SOURCE: &str = "../fo4-vm/examples/quest_run.rs";

fn once(text: &str, from: &str, to: &str, what: &str) -> String {
    let n = text.matches(from).count();
    if n != 1 {
        panic!("fo4-play: the patch point '{what}' occurs {n} times in {SOURCE}, expected once");
    }
    text.replacen(from, to, 1)
}

fn main() {
    println!("cargo:rerun-if-changed={SOURCE}");
    println!("cargo:rerun-if-changed=build.rs");
    let text = std::fs::read_to_string(SOURCE).expect("fo4-play: read quest_run.rs");
    // Inner attributes and module docs at the top cannot be included.
    let mut body = String::new();
    let mut head = true;
    for line in text.lines() {
        if head && (line.starts_with("#![") || line.starts_with("//!")) {
            continue;
        }
        head = false;
        body.push_str(line);
        body.push('\n');
    }
    let body = once(
        &body,
        "fn run() -> Result<(), E> {\n    let mut a: Vec<String> = std::env::args().skip(1).collect();\n",
        "fn run() -> Result<(), E> {\n    run_with(std::env::args().skip(1).collect())\n}\n\nfn run_with(mut a: Vec<String>) -> Result<(), E> {\n",
        "the arguments of run()",
    );
    let body = once(
        &body,
        "        let fired = clock_loop(\n",
        "        let fired = play::play_loop(\n            (m, vfs),\n",
        "the clock loop call of new_game()",
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("quest_run.rs");
    std::fs::write(out, body).expect("fo4-play: write the patched copy");
}
