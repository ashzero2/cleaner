# GPUI macOS compatibility patch

This directory contains `gpui-pre-macos` **0.3.10**, copied from the crates.io package used by `gpui-kit` **0.8.1**. The original package manifest, source and Apache-2.0 license are retained. The workspace uses `[patch.crates-io]` to select it; it is excluded from workspace membership.

The only changes to upstream Rust source are three expressions in `src/window.rs`:

```rust
.map_or_default(f)
```

becomes:

```rust
.map_or_else(Default::default, f)
```

The packaged source uses the unstable `result_option_map_or_default` API. These equivalent stable calls allow the dependency to compile without nightly or `RUSTC_BOOTSTRAP`. No window behavior or accessibility adapter code was changed.

Remove this directory and the workspace patch when a matched GPUI Kit/engine release builds on stable Rust without it. Upgrade the matched stack together, then rerun the bundled native E2E lane as well as the ordinary workspace checks.

Source: <https://crates.io/crates/gpui-pre-macos/0.3.10>. License: [LICENSE-APACHE](LICENSE-APACHE).
