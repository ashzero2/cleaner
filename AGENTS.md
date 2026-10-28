# cl-rs

Fast disk space analyzer and cleaner for macOS. A Rust core library shared by the CLI and native GUI; the GUI reuses the core without changing scanner semantics.

## Layout
- `crates/core` (`clrs-core`): scanner (`scan.rs`), immutable tree (`tree.rs`), saved scans (`cache.rs`, `snapshot.rs`), change journal (`journal.rs`), incremental patching (`update.rs`), cleanup rules (`rules.rs`, built-ins in `rules/default.toml`), cleanup actions (`cleanup.rs`), errors. No printing, no CLI concerns.
- `crates/cli` (`cl-rs`): arguments (`cli.rs`), commands (`commands.rs`), scan with spinner and Ctrl-C (`scan.rs`), text output (`report.rs`), JSON output (`json.rs`).
- macOS only. Raw syscalls and FFI (`libc`, FSEvents) live in `core/src/sys/`; nothing else uses `unsafe`.

## Commands
- `cargo fmt --all && cargo clippy --all-targets && cargo test` must be clean (zero warnings) before finishing.
- Benchmark: `cargo build --release && target/release/cl-rs scan ~ -d 0`. Baselines on 3.4M files (`/usr/bin/time -l`): cold scan ~14.5 s and ~435 MB peak RSS; repeat scan from the cache under 2 s and ~300 MB. Don't regress them.
- Approved GUI Phase 1 lives in `crates/gui` (`cl-rs-gui`): native GPUI Kit, read-only exploration, no cleanup execution. Build its development bundle with `sh scripts/bundle_gui.sh`; see `crates/gui/README.md` for native validation.
- Full-workspace checks: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`. For core/CLI-only iterations, use `-p clrs-core -p cl-rs`.

## Code style
- Rust 2024, rustfmt defaults. Lints are set in the workspace `Cargo.toml` (clippy `pedantic`); fix warnings instead of `#[allow]`ing them, unless there's a clear reason.
- No comments that restate the code. Doc comments only on public items whose name isn't self-explanatory. Every `unsafe` block needs a `// SAFETY:` comment.
- Add a helper or shared util only when two or more call sites need it, or it isolates unsafe/parsing logic. Otherwise keep code inline where it's used.
- Keep visibility as narrow as possible: `pub(crate)` by default, `pub` only for the core's API.
- Builder methods are `#[must_use]` and take `self`. Getters return references or `Copy` values.
- Names: no `get_` prefixes, no type names repeated in function names.

## Errors
- Core: typed `thiserror` enums, `#[non_exhaustive]`, with source errors chained via `#[source]`.
- CLI: `anyhow`, printed once in `main` as `error: {e:#}` with a non-zero exit code.
- No `unwrap()` outside tests; tests use `expect("reason")`. No panics on bad filesystem data: return or record an error.
- Unreadable paths are recorded as `ScanIssue` and the scan continues; they never abort it.

## Concurrency
- The scan runs on its own `rayon` pool (never the global one), so the caller's threads are unaffected.
- Shared state is `Send + Sync` without `unsafe`: atomics for counters (`Relaxed`), sharded `Mutex` for hot sets, `Mutex<Vec>` for rare events.
- Recover from a poisoned lock with `unwrap_or_else(PoisonError::into_inner)`.
- Per-thread scratch buffers use `thread_local!`; don't allocate in hot loops.
- File descriptors are `OwnedFd`, closed before recursing into children.
- `Tree` is immutable after build and shared via `Arc`; observers read `Progress` snapshots.

## Scanner semantics
- Size = allocated blocks (`ATTR_FILE_ALLOCSIZE`), not logical length.
- Hard links are counted once per `(dev, inode)`. Symlinks are never followed.
- Stays on the root's volume unless `cross_devices(true)` is set.
- Each directory owns one block: a packed name buffer plus an exact-size `Box<[Node]>` of its children, sorted largest first. Workers build and sort blocks during the scan; `Tree::build` only links them breadth-first (no copying). `NodeId` is `(block, index)`.
- Scan scratch buffers are per-thread and copied into exact-size boxes per directory; don't grow `Vec`s that outlive a directory.

## Saved scans and the change journal
- A snapshot (`~/Library/Caches/cl-rs/<hash>.snap`, override with `--cache-dir` / `CLRS_CACHE_DIR`) holds the tree, unreadable paths, the volume's FSEvents UUID, and the journal position. It is a cache: any problem loading, replaying or patching it falls back to a full scan.
- Writes are atomic (private temp file, fsync, rename, mode 0600, dir 0700) and skipped if another process holds the lock. Loading is streaming and validates every length, index and the CRC-32 before trusting anything.
- FSEvents conventions: per-device stream over the root's subtree; record the event id before a full scan; after replaying to `HistoryDone`, `FlushSync`, then take the checkpoint; dropped events, `RootChanged` and id wraparound force a full scan; `MustScanSubDirs` rescans recursively. The journal is advisory, so snapshots older than 3 days, or 7 days since the last full scan, are rebuilt.
- Incremental update: each changed directory is rescanned one level; unchanged subdirectories keep their blocks. Sizes are patched up the ancestors with checked arithmetic and any inconsistency falls back to a full scan. `Tree::updated` never mutates blocks shared with another `Tree`.
- Hard links: FSEvents may report an unlinked hard link under a surviving link's directory, so recorded links are `lstat`ed on every load. A new link to a file not yet tracked as linked cannot be placed, so it triggers a full scan.
- Cross-volume scans (`-x`) are never cached.

## Cleanup
- Rules are TOML (`id`, `name`, `category`, `safety = safe|review`, `paths`, optional `command`, optional `requires_sibling`). Paths start with `~/` or `/`; `*`/`?` match within a component, `**` spans directories but never enters `.app` bundles.
- Rules match against the scanned `Tree` (no extra disk reads). A rule never matches the scan root, nested matches fold into the enclosing one, and the first rule in file order owns a path.
- Default action is move to Trash via `NSFileManager`; never `rm` user data directly. A rule `command` (e.g. `npm cache clean --force`) runs instead when its program is installed, otherwise the path is trashed.
- `core::clean` refuses symlinks and paths fewer than 3 components deep. Safe items are only cleaned with `--safe`; review items need an explicit `--rule <id>`.
- `clean` needs `--yes` or a terminal prompt (never prompts with `--json`); `--dry-run` previews. Trashing does not free space until the Trash is emptied.
- Broad rules (e.g. `node_modules`) must use `requires_sibling` so they don't match global installs.

## CLI behaviour
- `Ctrl-C` during a scan cancels it (`Error::Cancelled`, exit 130); a second press exits immediately. After the scan, Ctrl-C exits 130.
- `--json` prints one pretty JSON document to stdout; progress, summary and hints stay on stderr.
- Unreadable paths with `EPERM` trigger the Full Disk Access hint (`ScanIssue::needs_full_disk_access`, `full_disk_access_needed` in JSON).

## Tests
- E2E only: no unit tests. CLI tests live in `crates/cli/tests/e2e.rs`, run the real `cl-rs` binary against a `tempfile` tree, and assert on its output. GUI E2E tests live in `crates/gui/tests/e2e.rs`, launch the real app against isolated fixture/config/cache directories, and drive native accessible controls. This separate opt-in lane needs a logged-in macOS session and Accessibility permission; ignored does not mean passed.
- New user-visible behaviour gets an e2e case. Tests must never run a non-dry-run `clean` (it would trash real files or run `npm`); use `--dry-run`.
- Rule tests set `HOME` to a `tempfile` dir so `~` resolves to the fixture. Tests that are not about the cache pass `--no-cache`; cache tests use a temp `CLRS_CACHE_DIR`.
- The key cache test is differential: mutate a tree, update from the snapshot, and require the JSON tree to equal a fresh scan. `scripts/fuzz_cache.py` does this with random operations on a real directory.
