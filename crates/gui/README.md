# cl-rs native GUI

Phase 1 is a **read-only** macOS storage workbench. It calls `clrs-core` directly; it does not spawn the CLI, duplicate the scanner, or execute cleanup actions.

## Build and run

Requires macOS, a Rust toolchain and Apple's Command Line Tools (including `swiftc` for native E2E tests).

```sh
sh scripts/bundle_gui.sh
open target/debug/cl-rs.app
```

To start at a particular folder:

```sh
open target/debug/cl-rs.app --args "$HOME/Downloads"
```

Use the `.app`, not the raw `target/debug/cl-rs-gui` executable. In local validation, the unbundled process returned an invalid native accessibility window hierarchy; the bundled executable exposed its window, controls and table correctly. The bundle script rebuilds the current source and checks its `Info.plist`.

`sh scripts/bundle_gui.sh release` creates a release-profile development bundle. These bundles are **unsigned local builds**, not signed/notarized distributions. The bundle declares macOS 15 as its minimum and the GUI manifest declares Rust 1.92; validation so far used macOS 26.7.1 and Rust 1.97.1, not those minimum versions.

## Included

- Native menus, folder picker, Home and ten recent locations.
- Background scans, live counts, Stop, Refresh, fresh scan, and cache reuse.
- Generation-checked results; previous completed results stay visible during a scan or after failure.
- Virtualized, largest-first directory table; parent/breadcrumb navigation and double-click to open folders.
- Inspector with exact allocated bytes, file counts, path, Finder reveal and path copying.
- System/light/dark appearance, comfortable/compact density and persisted window bounds.
- Scan Issues sheet and Full Disk Access guidance when macOS denies access.

No search, treemap, recommendations, arbitrary Trash or cleanup execution is included in Phase 1. Refresh currently returns to the scan root rather than restoring a nested selection.

### Keyboard

| Shortcut | Action |
| --- | --- |
| Cmd-O | Choose folder |
| Cmd-R | Refresh (reuse the cache when possible) |
| Cmd-Shift-R | Fresh scan |
| Cmd-. | Stop scan |
| Up / Down | Select table rows |
| Cmd-Down | Open selected folder |
| Cmd-Up | Enclosing folder within the scan |
| Cmd-I | Toggle inspector |
| Cmd-Shift-C | Copy selected/current path |
| Cmd-, | Settings |
| Cmd-Q | Quit and save settings |

Tab moves between focusable controls. The table retains keyboard focus when a focused result is replaced; settings use the component sheet's focus handling.

## Local data

- Preferences: `~/Library/Application Support/cl-rs/settings.json`.
- Saved scans: `~/Library/Caches/cl-rs`, shared with the CLI.
- `CLRS_GUI_CONFIG_DIR` and `CLRS_CACHE_DIR` override these directories for isolated runs.

Preference writes use a private, exclusive temporary file followed by rename. Invalid preferences fall back to defaults. Invalid/offscreen window bounds fall back to the centered default. Cross-volume scans never use saved scans. Non-Unicode paths remain inspectable and can be revealed in Finder, but cannot be copied exactly as Unicode text or persisted in recents.

## Validation

Ordinary checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The GUI E2E lane is ignored by default because it needs a logged-in macOS session and Accessibility/event-posting permission for the terminal or runner. It compiles a Swift Accessibility driver, packages the current Cargo-built executable in an isolated temporary `.app`, and uses isolated fixture, settings and cache directories.

```sh
cargo test -p cl-rs-gui --test e2e -- --ignored --test-threads=1 --nocapture
```

Run it serially and avoid typing into the app while it runs: the driver activates its own app and sends native key events. It does not enable permissions, automate System Settings, capture the desktop or execute cleanup. An ignored test is **not** a passing native test.

Currently passing native cases cover a cold scan and exact core allocation comparison, keyboard folder navigation, inspector toggling, appearance/density persistence, cached refresh and fresh-core comparison after mutation, and missing-root retry. These do not establish VoiceOver usability, Full Disk Access behavior, large-folder performance, or all cancellation/root-switch interleavings. Those remain Phase 1 acceptance work before declaring the phase fully validated.

Cargo currently reports an upstream future-compatibility advisory for `block` 0.1.6 (`static of uninhabited type`). This is separate from project Clippy warnings. See `cargo report future-incompatibilities`.

### App-owned scene capture

An optional development feature renders only this application's own GPUI scene to an image; it is not a desktop screenshot and does not require Screen Recording access.

```sh
sh scripts/bundle_gui.sh debug --features ui-capture
CLRS_GUI_CAPTURE="$PWD/target/gui.png" CLRS_GUI_CAPTURE_DELAY=5 \
  target/debug/cl-rs.app/Contents/MacOS/cl-rs-gui "$HOME/Downloads"
```

`CLRS_GUI_CAPTURE_DELAY` is an integer number of seconds (default 2). Choose enough time for the desired state to render; a timer does not guarantee scan completion. Wait for the PNG write to finish before closing the app. Normal builds omit capture support entirely.

## Implementation boundaries

`jobs.rs` serializes blocking scan/cache work on its own thread. `workbench.rs` owns UI state and rejects superseded generations. `explore.rs` formats visible table rows from an immutable tree. `settings.rs` handles bounded loading and private writes. The worker retains the core `Cached` value through its asynchronous save.

GPUI Kit 0.8.1 is pinned to its matching engine stack. The local macOS dependency patch changes only three unstable Option calls to stable equivalents; see [the patch notes](../../vendor/gpui-pre-macos/README.md). Core and CLI source and scanner semantics are unchanged.
