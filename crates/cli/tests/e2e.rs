use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

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

fn home_fixture() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    let root = home.path();
    write(&root.join(".npm/_cacache/a.bin"), 256 * 1024);
    write(&root.join(".npm/_npx/x/package.json"), 10);
    write(&root.join(".npm/_npx/x/node_modules/p/i.js"), 128 * 1024);
    write(
        &root.join("Library/Developer/Xcode/DerivedData/App/b.bin"),
        128 * 1024,
    );
    write(&root.join("proj/package.json"), 10);
    write(&root.join("proj/node_modules/pkg/i.js"), 64 * 1024);
    write(&root.join("proj/.next/c.bin"), 64 * 1024);
    write(&root.join("lone/node_modules/pkg/j.js"), 64 * 1024);
    home
}

fn cl_rs_home(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cl-rs"))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::null())
        .output()
        .expect("run cl-rs")
}

fn json(output: &Output) -> Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

#[test]
fn recommend_groups_findings_by_safety() {
    let home = home_fixture();
    let out = stdout(&cl_rs_home(&["recommend", "-q"], home.path()));

    let safe = out.find("Safe to remove").expect("safe group");
    let review = out.find("Needs review").expect("review group");
    assert!(safe < review, "{out}");
    assert!(out.contains("npm-cache") && out.contains("~/.npm"), "{out}");
    assert!(
        out.contains("build-output") && out.contains("~/proj/.next"),
        "{out}"
    );
    assert!(out.contains("xcode-derived-data"), "{out}");
    assert!(out.contains("~/proj/node_modules"), "{out}");
    assert!(
        !out.contains("lone/node_modules"),
        "needs package.json sibling:\n{out}"
    );
    assert!(
        !out.contains("_npx"),
        "nested match should fold into npm-cache:\n{out}"
    );
}

#[test]
fn recommend_json_lists_items_and_totals() {
    let home = home_fixture();
    let report = json(&cl_rs_home(&["recommend", "-q", "--json"], home.path()));

    let ids: Vec<&str> = report["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["id"].as_str().expect("id"))
        .collect();
    assert!(
        ids.contains(&"npm-cache") && ids.contains(&"node-modules"),
        "{ids:?}"
    );
    assert!(report["safe_bytes"].as_u64().expect("safe_bytes") > 0);
    assert!(report["review_bytes"].as_u64().expect("review_bytes") > 0);
    assert_eq!(report["full_disk_access_needed"], false);
}

#[test]
fn scan_json_has_tree_with_other_bucket() {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in ["a", "b", "c"] {
        write(&dir.path().join(name).join("f.bin"), 4096);
    }
    let report = json(&cl_rs(&["scan", "-q", "--json", "-n", "2"], dir.path()));
    assert_eq!(report["tree"]["kind"], "dir");
    assert_eq!(
        report["tree"]["children"]
            .as_array()
            .expect("children")
            .len(),
        2
    );
    assert_eq!(report["tree"]["other"]["count"], 1);
    assert_eq!(report["files"], 3);
}

#[test]
fn top_json_lists_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("small.bin"), 10);
    write(&dir.path().join("big.bin"), 64 * 1024);
    let report = json(&cl_rs(&["top", "-q", "--json", "-n", "1"], dir.path()));
    let files = report["files"].as_array().expect("files");
    assert_eq!(files.len(), 1);
    assert!(
        files[0]["path"]
            .as_str()
            .expect("path")
            .ends_with("big.bin")
    );
}

#[test]
fn clean_dry_run_previews_without_touching_files() {
    let home = home_fixture();
    let out = stdout(&cl_rs_home(
        &["clean", "--safe", "--dry-run", "-q"],
        home.path(),
    ));

    assert!(out.starts_with("Will clean"), "{out}");
    assert!(
        out.contains("npm-cache") && out.contains("build-output"),
        "{out}"
    );
    assert!(
        !out.contains("xcode-derived-data"),
        "review items need --rule:\n{out}"
    );
    assert!(home.path().join(".npm/_cacache/a.bin").exists());
    assert!(home.path().join("proj/.next/c.bin").exists());

    let plan = json(&cl_rs_home(
        &["clean", "--safe", "--dry-run", "-q", "--json"],
        home.path(),
    ));
    assert_eq!(plan["dry_run"], true);
}

#[test]
fn clean_dry_run_selects_review_rules_by_id() {
    let home = home_fixture();
    let out = stdout(&cl_rs_home(
        &["clean", "--rule", "xcode-derived-data", "--dry-run", "-q"],
        home.path(),
    ));
    assert!(
        out.contains("xcode-derived-data") && !out.contains("npm-cache"),
        "{out}"
    );
}

#[test]
fn clean_refuses_bad_or_unconfirmed_requests() {
    let home = home_fixture();
    let cases: [(&[&str], &str); 3] = [
        (&["clean", "-q"], "choose what to clean"),
        (&["clean", "--rule", "nope", "-q"], "unknown rule `nope`"),
        (&["clean", "--safe", "-q"], "without confirmation"),
    ];
    for (args, message) in cases {
        let output = cl_rs_home(args, home.path());
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
    assert!(home.path().join(".npm/_cacache/a.bin").exists());
}

#[test]
fn custom_rules_replace_builtin_ones() {
    let home = home_fixture();
    let rules = home.path().join("rules.toml");
    fs::write(
        &rules,
        "[[rule]]\nid = \"mine\"\nname = \"Mine\"\ncategory = \"x\"\nsafety = \"safe\"\npaths = [\"~/proj/.next\"]\n",
    )
    .expect("write rules");

    let out = stdout(&cl_rs_home(
        &["recommend", "-q", "--rules", rules.to_str().expect("utf-8")],
        home.path(),
    ));
    assert!(out.contains("mine") && !out.contains("npm-cache"), "{out}");
}

#[test]
fn invalid_rules_are_reported() {
    let home = home_fixture();
    let rules = home.path().join("bad.toml");
    fs::write(
        &rules,
        "[[rule]]\nid = \"x\"\nname = \"X\"\ncategory = \"c\"\nsafety = \"safe\"\npaths = [\"relative/path\"]\n",
    )
    .expect("write rules");

    let output = cl_rs_home(
        &["recommend", "-q", "--rules", rules.to_str().expect("utf-8")],
        home.path(),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid rule `x`"), "{stderr}");
}

#[test]
fn interrupt_stops_scan_with_exit_130() {
    let child = Command::new(env!("CARGO_BIN_EXE_cl-rs"))
        .args(["scan", "-q", "/System/Library"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cl-rs");
    thread::sleep(Duration::from_millis(500));
    let signalled = Instant::now();
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(status.success());

    let output = child.wait_with_output().expect("wait");
    assert_eq!(output.status.code(), Some(130));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cancelled"));
    assert!(signalled.elapsed() < Duration::from_secs(3));
}
