# GUI Phase 2 status

**Updated:** 2026-10-29

## Done

- **Multi-selection** (`selection.rs`): Shift range, Control toggle; staging set survives navigation and is cleared on a new scan. `staged_roots` drops items inside another staged item.
- **Treemap** (`treemap.rs`, `Workbench::treemap_panel`): squarified layout of the current folder (largest 150 entries); click selects the table row, double-click opens a folder, tooltip, selection highlight. The pane measures itself with a `canvas` and defers its refresh with `on_next_frame` (refreshing during prepaint can be lost).
- **History**: Back/Forward buttons, Cmd-[ / Cmd-]; reset on each scan.
- **Search** (`search.rs`): Cmd-F, 150 ms debounce, background thread with cancel flag, This folder / Entire scan, 500 results shown.
- **Results view**: search, recommendation groups and Largest Files (Cmd-L) reuse the table via `Directory::results`; the treemap hides while results are shown.
- **Recommendations** (`recommendations.rs`): built-in rules run in the background after each scan; sidebar lists up to 6 groups.
- **Staging + review**: "Staged" marker in the table, status-bar count, toolbar "Review (n)", review sheet (`panels::review`) showing path, size and Trash-vs-command action.
- **Packages**: `.app`/`.framework`/`.bundle`/`.plugin`/`.kext`/`.xpc` folders show as "Package" (double-click opens them).
- Fixed: double-clicking a table row never opened the folder (selection was toggled off).
- Fixed: `ReviewChanges` and `FreshScan` shared Cmd-Shift-R; Review is now Cmd-Shift-E.
- E2E (`crates/gui/tests/e2e.rs`, opt-in): treemap, search, back/forward, stage/review, and a recommendation group via a `HOME` fixture. `ax.swift` gained `find/back/forward/stage/review` keys and a `type` command. Text that tests must see needs an explicit `role` + `aria_label` (plain `div` text is not exposed).
- **Cleanup execution**: review sheet "Clean N Items" runs `clrs_core::clean_target` (new; `clean` delegates to it) per staged root on a background task, unstages successes, keeps failures staged with a warning, then rescans. Disabled while scanning/cleaning. E2E checks the button is offered but never presses it (a real clean would trash files).

## Not done

- Nested (multi-level) treemap; arrow-key navigation between tiles.
- Search filters (type, size, date).
- `selection.rs` still has placeholder unit tests; the project rule is e2e only.
