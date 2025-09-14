use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn spawn_qemu_and_stream(
    firmware: &PathBuf,
) -> (Child, Arc<Mutex<Vec<String>>>, Arc<Mutex<Vec<String>>>) {
    let mut child = Command::new("qemu-system-riscv64")
        .args(&[
            "-M",
            "virt",
            "-m",
            "32M",
            "-display",
            "none",
            "-serial",
            "mon:stdio",
            "-bios",
            firmware.to_str().unwrap(),
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
    let firmware = PathBuf::from("../target/riscv64imac-unknown-none-elf/debug/shadowfax-core");

    assert!(
        firmware.exists(),
        "firmware {} does not exist. Build it first or set FIRMWARE_PATH.",
        firmware.display()
    );

    let (mut child, out_lines, err_lines) = spawn_qemu_and_stream(&firmware);

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
