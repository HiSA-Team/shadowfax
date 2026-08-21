use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn spawn_qemu_and_stream(
    firmware: &Path,
    dtb: &Path,
    dice: &Path,
    dice_input_addr: &str,
) -> (Child, Arc<Mutex<Vec<String>>>, Arc<Mutex<Vec<String>>>) {
    let mut child = Command::new("qemu-system-riscv64")
        .args(&[
            "-M",
            "virt",
            "-m",
            "512M",
            "-nographic",
            "-smp",
            "1",
            "-bios",
            firmware.to_str().unwrap(),
            "-dtb",
            dtb.to_str().unwrap(),
            "-device",
            format!("loader,file={},addr={}", dice.display(), dice_input_addr).as_str(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn qemu");

    let out_lines = Arc::new(Mutex::new(Vec::new()));
    let err_lines = Arc::new(Mutex::new(Vec::new()));

    if let Some(stdout) = child.stdout.take() {
        let out_clone = Arc::clone(&out_lines);
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                println!("[qemu stdout] {}", line);
                let mut buf = out_clone.lock().unwrap();
                buf.push(line);
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        let err_clone = Arc::clone(&err_lines);
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                eprintln!("[qemu stderr] {}", line);
                let mut buf = err_clone.lock().unwrap();
                buf.push(line);
            }
        });
    }

    (child, out_lines, err_lines)
}

#[test]
fn firmware_boots_correctly() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let firmware = root.join("target/riscv64imac-unknown-none-elf/debug/shadowfax");
    let dtb = env::var_os("FDT_IMAGE")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() || path.exists() {
                path
            } else {
                PathBuf::from("../..").join(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from("../../bin/generic/device-tree.dtb"));
    let dice = PathBuf::from("../../bin/shadowfax.dice.bin");
    let output = Command::new("fdtget")
        .args([
            "-t",
            "x",
            dtb.to_str().unwrap(),
            "/chosen/shadowfax",
            "dice-input",
        ])
        .output()
        .expect("failed to read dice-input from the platform DTB");
    assert!(output.status.success());
    let cells: Vec<_> = String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .map(|cell| u64::from_str_radix(cell, 16).unwrap())
        .collect();
    let dice_input_addr = format!("0x{:x}", (cells[0] << 32) | cells[1]);

    assert!(
        firmware.exists(),
        "firmware {} does not exist. Build it first.",
        firmware.display()
    );
    assert!(
        dtb.exists(),
        "dtb {} does not exist. Build it first.",
        dtb.display()
    );
    assert!(
        dice.exists(),
        "dice payload {} does not exist. Build it first.",
        dice.display()
    );

    let (mut child, out_lines, err_lines) =
        spawn_qemu_and_stream(&firmware, &dtb, &dice, &dice_input_addr);

    let timeout = Duration::from_secs(60);
    let deadline = Instant::now() + timeout;
    let mut found = false;

    while Instant::now() < deadline {
        {
            let out = out_lines.lock().unwrap();
            if out.iter().any(|l| l.contains("OpenSBI")) {
                found = true;
                break;
            }
        }
        {
            let err = err_lines.lock().unwrap();
            if err.iter().any(|l| l.contains("OpenSBI")) {
                found = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }

    // try to terminate qemu cleanly
    let _ = child.kill();
    let _ = child.wait();

    if !found {
        // collect logs for the assertion message
        let out = out_lines.lock().unwrap().join("\n");
        let err = err_lines.lock().unwrap().join("\n");
        panic!(
            "Did not see 'OpenSBI' within {}s\n--- QEMU STDOUT ---\n{}\n--- QEMU STDERR ---\n{}\n",
            timeout.as_secs(),
            out,
            err,
        );
    }
}
