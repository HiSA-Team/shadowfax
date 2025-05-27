/*  The purpose of this file is to extend rust build process with out build steps
 *  which are:
 *      - link opensbi static library;
 *      - generate rust bindings from opensbi include;
 *      - specify correct linkerscript and define symbols depending on the platform
 *
 *  The idea of a build script is well documented here
 *  "https://doc.rust-lang.org/cargo/reference/build-scripts.html".
 *
 * The `build.rs` is executed on the build host and not on the target.
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
use std::env;
use std::path::PathBuf;

// Platform holds platform specific data needed at build time.
struct Platform<'a> {
    name: &'a str,
    fw_text_start_address: usize,
    fw_udom_payload_start_address: usize,
    fw_tdom_payload_start_address: usize,
}

// PLATFORMS describe all supported platform by shadowafax
const PLATFORMS: &[Platform] = &[Platform {
    name: "generic",
    fw_text_start_address: 0x80000000,
    fw_udom_payload_start_address: 0x80060000,
    fw_tdom_payload_start_address: 0x80100000,
}];

fn main() {
    // Sourcing `scripts/environment.sh` allow users to specify a PLATFORM (defaults to 'generic').
    // Retrieve platform details if exists otherwise throw an error
    let platform = env::var("PLATFORM").unwrap_or("generic".to_string());
    let platform = PLATFORMS
        .iter()
        .find(|v| v.name == platform.as_str())
        .unwrap_or_else(|| panic!("Unsupported platform: {platform}"));

    // Disable compiler optimization for now.
    println!("cargo:rustc=opt-level=0");

    // Define variables for linkerscript to make it parametric. The next instructions
    // populate the FW_TEXT_START and FW_PAYLOAD_START symbols in `linker.ld`
    println!(
        "cargo:rustc-link-arg=--defsym=FW_TEXT_START={}",
        platform.fw_text_start_address,
    );

    println!(
        "cargo:rustc-link-arg=--defsym=FW_UDOM_PAYLOAD_START={}",
        platform.fw_udom_payload_start_address,
    );

    println!(
        "cargo:rustc-link-arg=--defsym=FW_TDOM_PAYLOAD_START={}",
        platform.fw_tdom_payload_start_address,
    );

    // Tell the linker to use our linkerscript "linker.ld" and pass `-static` and `-nostdlib` flags
    println!("cargo:rustc-link-arg=-Tlinker.ld");
    println!("cargo:rustc-link-arg=-static");
    println!("cargo:rustc-link-arg=-nostdlib");

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

    // Rerun build.rs if one of these files changes.
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-changed=linker.ld");
    println!("cargo::rerun-if-changed=build.rs");
}
