use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};

fn write(path: &Path, len: usize) {
    fs::create_dir_all(path.parent().expect("has parent")).expect("create parent");
    fs::write(path, vec![1u8; len]).expect("write file");
}

fn cl_rs(args: &[&str], root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cl-rs"))
        .args(args)
        .arg(root)
        .output()
        .expect("run cl-rs")
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("utf-8 stdout")
}

#[test]
fn scan_lists_children_largest_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("big/a.bin"), 256 * 1024);
    write(&dir.path().join("big/nested/b.bin"), 64 * 1024);
    write(&dir.path().join("small/c.bin"), 10);

    let out = stdout(&cl_rs(&["scan", "-q", "-d", "2"], dir.path()));
    let big = out.find("big/").expect("big listed");
    let small = out.find("small/").expect("small listed");
    assert!(big < small, "{out}");
    assert!(out.contains("nested/"), "{out}");
    assert!(!out.contains("b.bin"), "depth limit ignored:\n{out}");
}

#[test]
fn summary_counts_files_and_hard_links_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let original = dir.path().join("a/original.bin");
    write(&original, 1_000_000);
    fs::create_dir_all(dir.path().join("b")).expect("mkdir");
    fs::hard_link(&original, dir.path().join("b/link.bin")).expect("hard link");

    let output = cl_rs(&["scan", "-d", "0"], dir.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("Scanned 2 files, 1.0 MB"), "{stderr}");
}

#[test]
fn top_orders_files_and_skips_symlink_targets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("a.bin"), 16 * 1024);
    write(&dir.path().join("deep/b.bin"), 128 * 1024);
    write(&dir.path().join("c.bin"), 64 * 1024);
    write(&outside.path().join("huge.bin"), 1024 * 1024);
    symlink(outside.path(), dir.path().join("link")).expect("symlink");

    let out = stdout(&cl_rs(&["top", "-q", "-n", "2"], dir.path()));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[0].ends_with("deep/b.bin"), "{out}");
    assert!(lines[1].ends_with("c.bin"), "{out}");
}

#[test]
fn unreadable_directory_is_reported_not_fatal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let locked = dir.path().join("locked");
    write(&locked.join("secret.bin"), 10);
    write(&dir.path().join("open.bin"), 10);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("chmod");

    let output = cl_rs(&["scan", "-d", "0"], dir.path());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("chmod");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("Scanned 1 files"), "{stderr}");
    assert!(stderr.contains("(1 unreadable)"), "{stderr}");
    assert!(stderr.contains("locked"), "{stderr}");
}

#[test]
fn missing_path_fails() {
    let output = cl_rs(&["scan", "-q"], Path::new("/definitely/not/here"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error: cannot open"));
}
