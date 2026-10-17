//! Boots the staged firmware under QEMU and checks that the platform comes up.
//!
//! Addresses come from the `layout.sh` that `scripts/layout.py` wrote next to
//! the device tree, so this test cannot drift from the platform it boots.
//! Run `make firmware` first; this test builds nothing.

use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Longest gap tolerated between two console lines.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Lines a healthy boot prints, in this order. The trusted-domain entry only
/// appears once Shadowfax has verified, loaded and initialized the TSM.
const MILESTONES: &[&str] = &[
    "Supervisor domains (DT order)",
    "trusted-domain [trusted, load TSM]",
    "untrusted-domain [boot]",
    "OpenSBI v",
    "Boot HART Domain",
];

fn layout_value(layout: &Path, key: &str) -> String {
    let text = std::fs::read_to_string(layout)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", layout.display()));
    text.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(name, _)| name.trim() == key)
        .map(|(_, value)| value.trim().trim_matches('\'').to_string())
        .unwrap_or_else(|| panic!("{key} is missing from {}", layout.display()))
}

fn existing(path: PathBuf, what: &str) -> PathBuf {
    assert!(
        path.exists(),
        "{what} {} is missing; run `make firmware` first",
        path.display()
    );
    path
}

#[test]
fn firmware_boots_correctly() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dtb = match env::var_os("FDT_IMAGE") {
        Some(path) => PathBuf::from(path),
        None => root.join("target/platform/generic/device-tree.dtb"),
    };
    let dtb = existing(dtb, "device tree");
    let layout = dtb.with_file_name("layout.sh");
    let artifacts = root.join("target/artifacts");

    let mut qemu = Command::new("qemu-system-riscv64");
    qemu.args(["-M", "virt", "-nographic", "-smp", "1"])
        .args(["-m", &layout_value(&layout, "SHADOWFAX_QEMU_MEM")])
        .arg("-bios")
        .arg(existing(
            root.join("target/riscv64imac-unknown-none-elf/debug/shadowfax"),
            "firmware",
        ))
        .arg("-dtb")
        .arg(&dtb);
    for (name, key) in [
        ("shadowfax.dice.bin", "SHADOWFAX_DICE_ADDR"),
        ("tsm.elf", "SHADOWFAX_TSM_IMAGE_ADDR"),
        ("tsm.bin.signature", "SHADOWFAX_TSM_SIGNATURE_ADDR"),
    ] {
        let file = existing(artifacts.join(name), name);
        qemu.arg("-device").arg(format!(
            "loader,file={},addr={},force-raw=on",
            file.display(),
            layout_value(&layout, key)
        ));
    }

    let mut child = qemu
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn qemu-system-riscv64");

    let stdout = child.stdout.take().expect("qemu stdout");
    let (lines, console) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            println!("[qemu] {line}");
            if lines.send(line).is_err() {
                break;
            }
        }
    });

    let mut pending = MILESTONES.iter();
    let mut expected = pending.next();
    let mut failure = None;
    while let Some(milestone) = expected {
        match console.recv_timeout(TIMEOUT) {
            Ok(line) if line.contains("panicked at") => {
                failure = Some(format!("firmware panicked while booting: {line}"));
                break;
            }
            Ok(line) => {
                if line.contains(milestone) {
                    expected = pending.next();
                }
            }
            Err(_) => {
                failure = Some(format!("console went quiet before printing {milestone:?}"));
                break;
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    if let Some(reason) = failure {
        panic!("{reason}");
    }
}
