// build.rs
use std::{env, fs, path::PathBuf};

fn stage_guest_payload(variable: &str, default: &str, output_name: &str) {
    println!("cargo:rerun-if-env-changed={variable}");

    let source = env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default));
    let source = source.canonicalize().unwrap_or_else(|error| {
        panic!(
            "failed to resolve {variable} payload {}: {error}",
            source.display()
        )
    });

    println!("cargo:rerun-if-changed={}", source.display());

    let destination =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set")).join(output_name);
    fs::copy(&source, &destination).unwrap_or_else(|error| {
        panic!(
            "failed to copy {variable} payload {} to {}: {error}",
            source.display(),
            destination.display()
        )
    });
}

fn main() {
    let linkerscript_path = PathBuf::from("memory.x").canonicalize().unwrap();

    // Put the linker script somewhere the linker can find it.
    println!("cargo:rustc-link-arg=-T{}", linkerscript_path.display());
    println!("cargo:rustc-link-arg=-static");
    println!("cargo:rustc-link-arg=-nostdlib");

    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_STANDALONE");

    if env::var_os("CARGO_FEATURE_STANDALONE").is_some() {
        stage_guest_payload("TSM_GUEST_ELF", "../linux/guest/vmlinux", "guest.elf");
        stage_guest_payload("TSM_GUEST_DTB", "../bin/linux-tvm.dtb", "guest.dtb");
        stage_guest_payload(
            "TSM_GUEST_INITRD",
            "../bin/linux-tvm-initramfs.cpio.gz",
            "guest.initrd",
        );
    }
}
