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
        .arg("--no-cache")
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
        .arg("--no-cache")
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
        .args(["scan", "-q", "--no-cache", "/System/Library"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cl-rs");
    thread::sleep(Duration::from_millis(1500));
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

const SETTLE: Duration = Duration::from_millis(1500);

fn cached(args: &[&str], root: &Path, cache: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cl-rs"))
        .args(args)
        .arg(root)
        .env("CLRS_CACHE_DIR", cache)
        .stdin(Stdio::null())
        .output()
        .expect("run cl-rs")
}

const FULL_TREE: [&str; 6] = ["scan", "-q", "--json", "-d", "99", "-n"];

fn full_tree(root: &Path, cache: &Path) -> Value {
    json(&cached(
        &[&FULL_TREE[..], &["10000000"]].concat(),
        root,
        cache,
    ))
}

fn fresh_tree(root: &Path) -> Value {
    let mut args = FULL_TREE.to_vec();
    args.extend(["10000000", "--no-cache"]);
    json(&cl_rs_plain(&args, root))
}

fn cl_rs_plain(args: &[&str], root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cl-rs"))
        .args(args)
        .arg(root)
        .output()
        .expect("run cl-rs")
}

fn assert_same_tree(actual: &Value, expected: &Value) {
    assert_eq!(
        actual["tree"], expected["tree"],
        "incremental tree differs from a fresh scan"
    );
    assert_eq!(actual["size"], expected["size"]);
    assert_eq!(actual["files"], expected["files"]);
}

fn snapshot_file(cache: &Path) -> std::path::PathBuf {
    fs::read_dir(cache)
        .expect("cache dir")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "snap"))
        .expect("snapshot file")
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(&root.join("a/f1.bin"), 8 * 1024);
    write(&root.join("a/f2.bin"), 100 * 1024);
    write(&root.join("b/c/d/g.bin"), 64 * 1024);
    write(&root.join("b/h.bin"), 4 * 1024);
    write(&root.join("big/x.bin"), 200 * 1024);
    write(&root.join("swap/file"), 10);
    write(&root.join("keep/deep/er/still.bin"), 32 * 1024);
    write(&root.join("emptyme/one.bin"), 16 * 1024);
    dir
}

#[test]
fn second_scan_loads_the_saved_snapshot() {
    let dir = project();
    let cache = tempfile::tempdir().expect("cache dir");

    let first = full_tree(dir.path(), cache.path());
    assert_eq!(first["source"], "scan");
    let second = full_tree(dir.path(), cache.path());
    assert_eq!(second["source"], "cache");
    assert_eq!(second["changed_dirs"], 0);
    assert_same_tree(&second, &first);

    let mode = fs::metadata(snapshot_file(cache.path()))
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "snapshot must be private, mode {mode:o}");
}

#[test]
fn incremental_update_matches_a_fresh_scan() {
    let dir = project();
    let cache = tempfile::tempdir().expect("cache dir");
    let root = dir.path();
    assert_eq!(full_tree(root, cache.path())["source"], "scan");

    write(&root.join("a/new.bin"), 50 * 1024);
    fs::remove_file(root.join("a/f1.bin")).expect("remove file");
    write(&root.join("b/h.bin"), 300 * 1024);
    write(&root.join("n1/n2/n3/deep.bin"), 80 * 1024);
    fs::remove_dir_all(root.join("big")).expect("remove dir");
    fs::rename(root.join("b/c"), root.join("b/c2")).expect("rename dir");
    fs::remove_file(root.join("swap/file")).expect("remove file");
    write(&root.join("swap/file/inner.bin"), 20 * 1024);
    fs::remove_dir_all(root.join("emptyme")).expect("remove dir");
    fs::create_dir(root.join("emptyme")).expect("recreate empty dir");
    fs::create_dir(root.join("brand-new-empty")).expect("empty dir");
    thread::sleep(SETTLE);

    let updated = full_tree(root, cache.path());
    assert_eq!(updated["source"], "cache");
    assert!(updated["changed_dirs"].as_u64().expect("changed_dirs") > 0);
    assert_same_tree(&updated, &fresh_tree(root));

    let again = full_tree(root, cache.path());
    assert_eq!(again["changed_dirs"], 0);
    assert_same_tree(&again, &updated);
}

#[test]
fn repeated_updates_stay_consistent() {
    let dir = project();
    let cache = tempfile::tempdir().expect("cache dir");
    let root = dir.path();
    full_tree(root, cache.path());

    for round in 0..4 {
        write(
            &root.join(format!("a/round{round}.bin")),
            (round + 1) * 10 * 1024,
        );
        write(&root.join(format!("r{round}/sub/leaf.bin")), 4096);
        if round > 0 {
            fs::remove_dir_all(root.join(format!("r{}", round - 1))).expect("remove dir");
        }
        thread::sleep(SETTLE);
        let updated = full_tree(root, cache.path());
        assert_eq!(updated["source"], "cache", "round {round}");
        assert_same_tree(&updated, &fresh_tree(root));
    }
}

#[test]
fn hard_link_changes_keep_totals_exact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = tempfile::tempdir().expect("cache dir");
    let root = dir.path();
    write(&root.join("a/orig.bin"), 256 * 1024);
    fs::create_dir_all(root.join("b")).expect("mkdir");
    fs::hard_link(root.join("a/orig.bin"), root.join("b/link.bin")).expect("link");
    write(&root.join("c/other.bin"), 16 * 1024);
    full_tree(root, cache.path());

    fs::create_dir_all(root.join("d")).expect("mkdir");
    fs::hard_link(root.join("b/link.bin"), root.join("d/link2.bin")).expect("link");
    fs::remove_file(root.join("a/orig.bin")).expect("remove original");
    thread::sleep(SETTLE);
    let updated = full_tree(root, cache.path());
    assert_eq!(updated["source"], "cache");
    let fresh = fresh_tree(root);
    assert_eq!(updated["size"], fresh["size"], "hard link charged once");
    assert_eq!(updated["files"], fresh["files"]);

    fs::remove_file(root.join("b/link.bin")).expect("remove link");
    fs::remove_file(root.join("d/link2.bin")).expect("remove last link");
    thread::sleep(SETTLE);
    let emptied = full_tree(root, cache.path());
    assert_eq!(emptied["size"], fresh_tree(root)["size"]);
}

#[test]
fn damaged_snapshots_fall_back_to_a_full_scan() {
    let dir = project();
    let cache = tempfile::tempdir().expect("cache dir");
    let expected = full_tree(dir.path(), cache.path());
    let snapshot = snapshot_file(cache.path());
    let original = fs::read(&snapshot).expect("read snapshot");

    let mut flipped = original.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0xff;
    let damaged: [(&str, Vec<u8>); 4] = [
        ("flipped byte", flipped),
        ("truncated", original[..original.len() / 2].to_vec()),
        ("garbage", b"definitely not a snapshot".to_vec()),
        ("empty", Vec::new()),
    ];
    for (label, bytes) in damaged {
        fs::write(&snapshot, bytes).expect("damage snapshot");
        let recovered = full_tree(dir.path(), cache.path());
        assert_eq!(recovered["source"], "scan", "{label}");
        assert_same_tree(&recovered, &expected);
        assert_eq!(
            full_tree(dir.path(), cache.path())["source"],
            "cache",
            "{label}: repaired"
        );
    }
}

#[test]
fn fresh_and_no_cache_flags_control_the_snapshot() {
    let dir = project();
    let cache = tempfile::tempdir().expect("cache dir");

    let output = cached(
        &["scan", "-q", "--json", "--no-cache"],
        dir.path(),
        cache.path(),
    );
    assert_eq!(json(&output)["source"], "scan");
    assert_eq!(
        fs::read_dir(cache.path()).expect("cache dir").count(),
        0,
        "--no-cache wrote files"
    );

    full_tree(dir.path(), cache.path());
    let output = cached(
        &["scan", "-q", "--json", "--fresh"],
        dir.path(),
        cache.path(),
    );
    assert_eq!(json(&output)["source"], "scan");
    assert_eq!(full_tree(dir.path(), cache.path())["source"], "cache");
}

#[test]
fn unreadable_directories_are_remembered_and_rechecked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = tempfile::tempdir().expect("cache dir");
    let root = dir.path();
    write(&root.join("open/file.bin"), 4096);
    write(&root.join("locked/secret.bin"), 32 * 1024);
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o000)).expect("chmod");

    let first = full_tree(root, cache.path());
    let again = full_tree(root, cache.path());
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o755)).expect("chmod");
    assert_eq!(first["unreadable"].as_array().expect("issues").len(), 1);
    assert_eq!(again["source"], "cache");
    assert_eq!(
        again["unreadable"].as_array().expect("issues").len(),
        1,
        "issue not remembered"
    );

    thread::sleep(SETTLE);
    let opened = full_tree(root, cache.path());
    assert!(opened["unreadable"].as_array().expect("issues").is_empty());
    assert_same_tree(&opened, &fresh_tree(root));
}

#[test]
fn clean_dry_run_uses_the_snapshot_too() {
    let home = home_fixture();
    let cache = tempfile::tempdir().expect("cache dir");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_cl-rs"))
            .args(["recommend", "-q", "--json"])
            .env("HOME", home.path())
            .env("CLRS_CACHE_DIR", cache.path())
            .stdin(Stdio::null())
            .output()
            .expect("run cl-rs")
    };
    let first = json(&run());
    let second = json(&run());
    assert_eq!(second["source"], "cache");
    assert_eq!(first["items"], second["items"]);
}
