# cl-rs

Fast disk space analyzer and cleaner for macOS. A Rust core library with a CLI on top; a GUI comes later and must reuse the core unchanged.

## Layout
- `crates/core` (`clrs-core`): scanner, immutable tree, errors. No printing, no CLI concerns.
- `crates/cli` (`cl-rs`): argument parsing (`cli.rs`), output formatting (`report.rs`), orchestration (`main.rs`).
- macOS only. Raw syscalls live in `core/src/sys.rs`; nothing else touches `libc`.

## Commands
- `cargo fmt --all && cargo clippy --all-targets && cargo test` must be clean (zero warnings) before finishing.
- Benchmark: `cargo build --release && target/release/cl-rs scan ~ -d 0`. Baseline ~14.5 s, 3.4M files. Don't regress it.

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
- Tree nodes are stored breadth-first in a flat `Vec`, names are packed into one byte buffer, `NodeId` is a `u32`, and children are sorted largest first.

## Cleanup (upcoming)
- Default action is move to Trash; never `rm` user data directly. Prefer the tool's own command (`npm cache clean`, `ollama rm`, `xcrun simctl`).
- Every rule is marked as either safe or needs review.

## Tests
- E2E only: no unit tests. Tests live in `crates/cli/tests/e2e.rs`, run the real `cl-rs` binary against a `tempfile` tree, and assert on its output.
- New user-visible behaviour gets an e2e case.
