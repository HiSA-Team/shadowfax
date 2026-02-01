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
#![feature(stmt_expr_attributes)]
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

const PLATFORM_BASE_DIR: &str = "platform";
const LINKERSCRIPT_PATH: &str = "memory.x";

fn main() {
    // Ensure the bin/ folder exists.
    let bin_dir = PathBuf::from("../bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let opensbi_path = PathBuf::from("opensbi");

    // Sourcing `environment.sh` allows users to specify a PLATFORM (defaults to 'generic').
    // Retrieve platform details if exists otherwise throw an error
    let platform = env::var("PLATFORM").unwrap_or_else(|_| "generic".to_string());

    let platform_dir = PathBuf::from(PLATFORM_BASE_DIR).join(&platform);

    // Setup linker:
    // - build and link opensbi
    // - link linkerscript
    {
        let status = std::process::Command::new("make")
            .args([
                "-C",
                &opensbi_path.to_string_lossy(),
                &format!("PLATFORM={}", &platform),
                &format!(
                    "CROSS_COMPILE={}",
                    &env::var("CROSS_COMPILE").expect("CROSS_COMPILE not set")
                ),
            ])
            .status()
            .expect("failed to build opensbi");
        if !status.success() {
            panic!("OpenSBI build failed with status: {}", status);
        }

        let linkerscript_path = PathBuf::from(LINKERSCRIPT_PATH).canonicalize().unwrap();
        let libopensbi_path = opensbi_path
            .join(format!("build/platform/{}/lib", &platform))
            .canonicalize()
            .unwrap();

        configure_linker(&linkerscript_path, &libopensbi_path);

        // recompile if linkerscript changes
        println!("cargo::rerun-if-changed={}", &libopensbi_path.display());
    }

    // Compile the device tree
    {
        let dts_file = &platform_dir.join("device-tree.dts").canonicalize().unwrap();
        let dtb_file = &bin_dir.join("device-tree.dtb");
        let status = Command::new("dtc")
            .args([
                "-I",
                "dts",
                "-O",
                "dtb",
                "-o",
                &dtb_file.to_str().unwrap(),
                &dts_file.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to execute dtc");
        assert!(status.success(), "dtc failed with exit status: {status}");

        // recompile if the device tree changes
        println!("cargo::rerun-if-changed={}", dts_file.display());
    }

    // Generate rust bindgen
    {
        let include_path = opensbi_path.join("include").canonicalize().unwrap();

        // Use bindgen API to create a valid `bindings.rs` which will be used
        // to create the `opensbi` module in `main.rs`. This is taken from
        // https://rust-lang.github.io/rust-bindgen/library-usage.html
        let bindings = bindgen::Builder::default()
            // we need to use core because bare metal programs do not have
            // the std library.
            .use_core()
            // pass our `wrapper.h`
            .header("wrapper.h")
            .clang_arg("-I")
            .clang_arg(include_path.to_string_lossy())
            //TODO: make the target architecture variable
            .clang_args([
                "-mabi=lp64",
                "-march=rv64imac",
                "--target=riscv64-unknown-elf",
            ])
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

        // recompile if wrapper.h changes
        println!("cargo::rerun-if-changed=wrapper.h");
    }

    println!("cargo::rerun-if-changed=build.rs");
}

fn configure_linker(linkerscript_path: &PathBuf, libopensbi_path: &PathBuf) {
    // Tell the linker to use our linkerscript "linker.ld" and pass `-static` and `-nostdlib` flags
    #[rustfmt::skip]
    println!("cargo:rustc-link-arg=-T{}", linkerscript_path.display());
    println!("cargo:rustc-link-arg=-static");
    println!("cargo:rustc-link-arg=-nostdlib");
    println!("cargo:rustc-link-search={}", libopensbi_path.display());

    // Opensbi installs the static library in `./lib64/lp64/opensbi/generic/lib/`
    // and calls it `libplatsbi.a`. The linker automatically adds the `lib` prefix
    // and `.a` suffix.
    println!("cargo:rustc-link-lib=platsbi");
}
