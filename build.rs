//! The action table is generated from the player's own copy of the game and is
//! not part of the repository. Say so plainly instead of letting the compiler
//! fail on a missing module.

use std::path::Path;

const GENERATED: &str = "src/sim/extracted.rs";

fn main() {
    println!("cargo:rerun-if-changed={GENERATED}");
    if !Path::new(GENERATED).exists() {
        eprintln!();
        eprintln!("{GENERATED} is missing.");
        eprintln!("It is generated from your own Elden Ring files and is not in the repository.");
        eprintln!("Follow the Setup section of README.md, then run:");
        eprintln!();
        eprintln!("    python tools/setup.py");
        eprintln!();
        std::process::exit(1);
    }
}
