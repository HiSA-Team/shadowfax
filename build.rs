/* This code generates Rust FFI bindings from opensbi include directory and it is
 * taken partially taken from https://rust-lang.github.io/rust-bindgen/tutorial-3.html.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
use std::env;
use std::path::PathBuf;

fn main() {
    let libdir_path = PathBuf::from("./lib64/lp64/opensbi/generic/lib/")
        .canonicalize()
        .unwrap();

    println!("cargo:rustc-link-search={}", libdir_path.to_str().unwrap());
    println!("cargo:rustc-link-lib=platsbi");

    if cfg!(target_env = "musl") {
        println!("cargo:rustc-linker=riscv64-linux-musl-ld");
    } else {
        println!("cargo:rustc-linker=riscv64-linux-gnu-ld");
    }

    let bindings = bindgen::Builder::default()
        .use_core()
        .header("wrapper.h")
        .clang_arg("-Iinclude/")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
