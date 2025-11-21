// build.rs
use std::path::PathBuf;

fn main() {
    let linkerscript_path = PathBuf::from("memory.x").canonicalize().unwrap();

    // Put the linker script somewhere the linker can find it.
    println!("cargo:rustc-link-arg=-T{}", linkerscript_path.display());
    println!("cargo:rustc-link-arg=-static");
    println!("cargo:rustc-link-arg=-nostdlib");

    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
