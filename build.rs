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

    // Tell cargo to tell rustc to link our `hello` library. Cargo will
    // automatically know it must look for a `libhello.a` file.
    println!("cargo:rustc-link-lib=platsbi");
    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        .use_core()
        // The input header we would like to generate
        // bindings for.
        .header("wrapper.h")
        // pass include directory where opensbi is installed
        .clang_arg("-Iinclude/")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
