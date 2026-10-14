//! The Python runtime sources are compiled into this crate (see `src/python.rs`).
//! Tell Cargo to rebuild when any of them changes, including newly added files.

fn main() {
    println!("cargo:rerun-if-changed=../../runtime/python");
}
