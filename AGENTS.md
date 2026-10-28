# cl-rs

Fast disk space analyzer and cleaner for macOS. A Rust core library with a CLI on top; a GUI comes later and must reuse the core unchanged.

## Layout
- `crates/core` (`clrs-core`): scanner, immutable tree, cleanup rules (`rules.rs`, built-ins in `rules/default.toml`), cleanup actions (`cleanup.rs`), errors. No printing, no CLI concerns.
- `crates/cli` (`cl-rs`): arguments (`cli.rs`), commands (`commands.rs`), scan with spinner and Ctrl-C (`scan.rs`), text output (`report.rs`), JSON output (`json.rs`).
- macOS only. Raw syscalls live in `core/src/sys.rs`; nothing else touches `libc`.

## Commands
- `cargo fmt --all && cargo clippy --all-targets && cargo test` must be clean (zero warnings) before finishing.
- Benchmark: `cargo build --release && target/release/cl-rs scan ~ -d 0`. Baseline ~14.5 s and ~385 MB peak RSS on 3.4M files (`/usr/bin/time -l`). Don't regress either.

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
- E2E only: no unit tests. Tests live in `crates/cli/tests/e2e.rs`, run the real `cl-rs` binary against a `tempfile` tree, and assert on its output.
- New user-visible behaviour gets an e2e case. Tests must never run a non-dry-run `clean` (it would trash real files or run `npm`); use `--dry-run`.
- Rule tests set `HOME` to a `tempfile` dir so `~` resolves to the fixture.
