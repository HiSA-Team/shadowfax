/*  The purpose of this file is to extend rust build process with out build steps
 *  which are:
 *      - link opensbi static library;
 *      - generate rust bindings from opensbi include;
 *
 *  The idea of a build script is well documented here
 *  "https://doc.rust-lang.org/cargo/reference/build-scripts.html".
 *
 * The `build.rs` is executed on the build host ant not on the target.
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
use std::env;
use std::path::PathBuf;

fn main() {
    // Disable compiler optimization for now.
    println!("cargo:rustc=opt-level=0");

    // Tell the linker to use our linkerscript "linker.ld" and pass `--static` flag
    println!("cargo:rustc-link-arg=-Tlinker.ld");
    println!("cargo:rustc-link-arg=-static");

    // Link the openbsi platform library. We specify the opensbi installation path
    // (by default this is obtained from `make PLATFORM=generic install I=<path-to-shadowfax>`)
    let libdir_path = PathBuf::from("./lib64/lp64/opensbi/generic/lib/")
        .canonicalize()
        .unwrap();

    println!("cargo:rustc-link-search={}", libdir_path.to_str().unwrap());

    // Opensbi installs the static library in `./lib64/lp64/opensbi/generic/lib/`
    // and calls it `libplatsbi.a`. The linker automatically adds the `lib` prefix
    // and `.a` suffix.
    println!("cargo:rustc-link-lib=platsbi");

    // Use bindgen API to create a valid `bindings.rs` which will be used
    // to create the `opensbi` module in `main.rs`. This is taken from
    // https://rust-lang.github.io/rust-bindgen/library-usage.html
    let bindings = bindgen::Builder::default()
        // we need to use core because bare metal programs do not have
        // the std library.
        .use_core()
        // pass our `wrapper.h`
        .header("wrapper.h")
        // this is the include directory installed from opensbi using the
        // command `make PLATFORM=generic install I=<path-to-shadowfax>`
        .clang_arg("-Iinclude/")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    // save the bindings in the build directory
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
