/*  The purpose of this file is to extend rust build process with out build steps
 *  which are:
 *      - link opensbi static library;
 *      - generate rust bindings from opensbi include;
 *      - specify correct linkerscript;
 *      - compile the device tree;
 *
 *  The idea of a build script is well documented here
 *  "https://doc.rust-lang.org/cargo/reference/build-scripts.html".
 *
 * The `build.rs` is executed on the build host and not on the target.
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

const PLATFORM_BASE: &str = "platform";

fn main() {
    // Ensure the bin/ folder exists.
    fs::create_dir_all("bin").unwrap();
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let opensbi_path = env::var("OPENSBI_PATH").map(PathBuf::from).expect("OPENSBI_PATH must be set; run `source environment.sh <opensbi-path>` or set OPENSBI_PATH in your environment");

    // Sourcing `scripts/environment.sh` allow users to specify a PLATFORM (defaults to 'generic').
    // Retrieve platform details if exists otherwise throw an error
    let platform = env::var("PLATFORM").unwrap_or_else(|_| "generic".to_string());

    // write the selected linkerscript where the rust can find it
    let platform_dir = PathBuf::from(PLATFORM_BASE).join(&platform);
    let content = fs::read(platform_dir.join("memory.x")).unwrap();

    // save linkerscript where we can find it.
    fs::write(out_path.join("memory.x"), content).unwrap();

    // compile the device tree
    let dts_file = platform_dir.join("device-tree.dts");
    let dtb_file = "bin/device-tree.dtb";
    let status = Command::new("dtc")
        .args([
            "-I",
            "dts",
            "-O",
            "dtb",
            "-o",
            dtb_file,
            dts_file.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute dtc");

    assert!(status.success(), "dtc failed with exit status: {status}");

    // Disable compiler optimization for now.
    println!("cargo:rustc=opt-level=0");

    // Tell the linker to use our linkerscript "linker.ld" and pass `-static` and `-nostdlib` flags
    #[rustfmt::skip]
    println!("cargo:rustc-link-arg=-T{}", out_path.join("memory.x").display());
    println!("cargo:rustc-link-arg=-static");
    println!("cargo:rustc-link-arg=-nostdlib");
    println!("cargo:rustc-link-arg=-melf64lriscv");
    println!("cargo:rustc-link-arg=-Map=linker.map");

    // Link the openbsi platform library. We specify the opensbi installation path
    // (by default this is obtained from `make PLATFORM=generic install I=<path-to-shadowfax>`)
    let libdir_path = opensbi_path
        .join(format!("build/platform/{}/lib", &platform))
        .canonicalize()
        .unwrap();

    let include_path = opensbi_path.join("include").canonicalize().unwrap();

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
        .clang_arg("-I")
        .clang_arg(include_path.to_string_lossy())
        .derive_debug(true)
        .derive_default(true)
        .ctypes_prefix("::core::ffi")
        .generate()
        .expect("Unable to generate bindings");

    // save the bindings in the build directory
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    // Rerun build.rs if one of these files changes.
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-changed=build.rs");
    #[rustfmt::skip]
    println!("cargo::rerun-if-changed={}", out_path.join("memory.x").display());
    println!("cargo::rerun-if-changed={}", dts_file.display());
}
