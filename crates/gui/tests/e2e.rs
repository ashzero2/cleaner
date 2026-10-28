use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct App {
    child: Child,
    driver: PathBuf,
}

impl App {
    fn launch(root: &Path, fixture: &Path) -> Self {
        let contents = fixture.join("cl-rs.app/Contents");
        let executable = contents.join("MacOS/cl-rs-gui");
        fs::create_dir_all(executable.parent().expect("bundle directory")).expect("create bundle");
        fs::copy(env!("CARGO_BIN_EXE_cl-rs-gui"), &executable).expect("copy current binary");
        fs::write(contents.join("Info.plist"), include_bytes!("../Info.plist"))
            .expect("write bundle identity");
        let driver = fixture.join("ax-driver");
        let compiled = Command::new("swiftc")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ax.swift"))
            .arg("-o")
            .arg(&driver)
            .output()
            .expect("compile native driver");
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let child = Command::new(executable)
            .arg(root)
            .env("CLRS_GUI_CONFIG_DIR", fixture.join("config"))
            .env("CLRS_CACHE_DIR", fixture.join("cache"))
            .stdout(Stdio::null())
            .spawn()
            .expect("launch bundled GUI");
        Self { child, driver }
    }

    fn drive(&self, command: &str, value: &str) -> String {
        let output = Command::new(&self.driver)
            .arg(self.child.id().to_string())
            .args([command, value])
            .output()
            .expect("run native accessibility driver");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "AX command {command} {value} failed:\n{stdout}\n{stderr}"
        );
        String::from_utf8(output.stdout).expect("UTF-8 driver output")
    }

    fn quit(&mut self) {
        self.drive("key", "quit");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().expect("query child status") {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "app did not quit");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "needs a logged-in macOS session and Accessibility permission for the test runner"]
fn scan_navigate_settings_and_refresh() {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path().join("scan-root");
    let nested = root.join("Projects");
    fs::create_dir_all(&nested).expect("nested folder");
    fs::write(nested.join("unique-nested-file.txt"), vec![42; 16_384]).expect("fixture file");
    fs::write(root.join("readme.txt"), b"fixture").expect("root file");
    std::os::unix::fs::symlink(&nested, root.join("shortcut")).expect("symlink");
    let mut app = App::launch(&root, fixture.path());
    app.drive("wait", "Full scan");
    app.drive("wait", "Projects");
    app.drive("wait", "Symlink");
    let (tree, _) = clrs_core::Scanner::new(&root)
        .run()
        .expect("reference scan");
    let expected = tree.node(tree.root()).size();
    app.drive("wait", &format!("{expected} bytes)"));
    app.drive("key", "down");
    app.drive("key", "open");
    app.drive("wait", "unique-nested-file.txt");
    app.drive("key", "up");
    app.drive("wait", "readme.txt");
    app.drive("press", "Hide Inspector");
    app.drive("wait", "Show Inspector");
    app.drive("press", "Show Inspector");
    app.drive("key", "settings");
    app.drive("wait", "Reuse saved scans");
    app.drive("press", "Dark");
    app.drive("press", "Compact");
    app.drive("press", "Done");
    fs::write(root.join("new-after-refresh.txt"), b"new file").expect("mutate fixture");
    app.drive("key", "refresh");
    app.drive("wait", "new-after-refresh.txt");
    app.drive("wait", "Updated saved scan");
    let (tree, _) = clrs_core::Scanner::new(&root)
        .run()
        .expect("fresh reference scan");
    app.drive("wait", &format!("{} bytes)", tree.node(tree.root()).size()));
    app.quit();
    let saved: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.path().join("config/settings.json")).expect("saved settings"),
    )
    .expect("valid settings JSON");
    assert_eq!(saved["appearance"], "Dark");
    assert_eq!(saved["density"], "Compact");
}

#[test]
#[ignore = "needs a logged-in macOS session and Accessibility permission for the test runner"]
fn failed_root_can_be_retried() {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path().join("missing-root");
    let mut app = App::launch(&root, fixture.path());
    app.drive("wait", "Scan failed");
    fs::create_dir(&root).expect("restore missing folder");
    fs::write(root.join("recovered.txt"), b"available again").expect("fixture file");
    app.drive("press", "Retry");
    app.drive("wait", "recovered.txt");
    app.drive("wait", "Full scan");
    app.quit();
}
