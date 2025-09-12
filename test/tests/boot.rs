use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
fn firmware_boots_correctly() {
    // ensure secure-firmware is built by depending on it (or build manually before running tests)
    let firmware = PathBuf::from("../target/riscv64imac-unknown-none-elf/debug/shadowfax-core");

    assert!(
        firmware.exists(),
        "firmware {} does not exists. You need to build it first",
        firmware.display()
    );

    // Spawn a qemu process
    let mut child = Command::new("qemu-system-riscv64")
        .args(&[
            "-M",
            "virt",
            "-m",
            "32M",
            "-nographic",
            "-bios",
            firmware.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn qemu");

    // simple timeout: give the binary some time to print
    thread::sleep(Duration::from_secs(2));

    // try to stop qemu (ignore errors)
    let _ = child.kill();

    let out = child.wait_with_output().expect("waiting on qemu failed");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.contains("OpenSBI"), "qemu stdout:\n{}", stdout);
}
