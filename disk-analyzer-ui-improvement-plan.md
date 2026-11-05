# Disk Analyzer — Incremental UI/UX Plan (v2)

**Version:** 2.0
**Supersedes:** v1 of this file
**Scope:** `crates/gui` (cl-rs-gui) and the cleanup guards in `crates/core`.

## 1. Goal

Make cl-rs-gui feel like one app: one control grammar, one type scale, one dialog
grammar, and cleanup that cannot destroy protected artifacts. Keep the current one-window
Miller-column explorer and the ring map; do not redesign navigation.

## 2. Non-goals

- No framework migration, backend rewrite, or new navigation model.
- No palette change. `Palette` in `theme.rs` and the `DESIGN.md` contrast table stay.
- No replacement of the ring map, and no second chart type.
- No resizable chart/list split; the map toggle is the only view control.
- No new unit-test lane. New user-visible behaviour gets an E2E case.

## 3. Root cause (measured 2026-11-05)

- gpui-kit `Button` gives three sizes: xsmall 20, small 24, medium/large 32, all at
  `theme.radius` (set to 9 in `theme::apply`). The app then hand-sets heights 18, 20, 24,
  28, 30, 32, 38, 42, 52, 54, 68 and radii 3, 9, 10, 12, 14, 999.
- Buttons that read as chips:
  - `chrome.rs:97` — Locked is a 32 px `Button` with `rounded(999)`, same silhouette as a
    safety chip (20 px) and a tray chip (28 px).
  - `panels.rs:518` — Appearance/Density are rows of `.small().ghost().selected()` buttons
    (24 px), while the search scope is `.xsmall()` (20 px, `explorer.rs:1208`).
  - `tray.rs:88` — a 28 px pill wraps a 20 px pill button.
- One semantic action at three sizes: row `+` 24, detail "Add to tray" 24, "Add all" 20,
  "Review N items" 32. Icon-only actions: 32 toolbar, 24 settings gear, 20 dismiss.
- Dialogs: review 580, issues 580, settings 480, done 480. The sheet shrinks 100 px when
  cleanup finishes. Footers use three different grammars.
- `Density` is persisted and exposed but only changes the table row (42/38,
  `explorer.rs:1109`). Everything else ignores it.
- gpui-component already ships a real connected control: `TabBar::new().segmented()`
  (inner 24 default / 28 at `Large`), and `setting::{SettingPage, SettingGroup,
  SettingItem}`. Do not hand-roll either.

## 4. The grammar (authoritative)

### 4.1 Control ladder

| Token | Height | Use |
|---|---|---|
| `CONTROL_SM` | 24 | icon-only actions in dense rows (add, dismiss, remove, tray ×), and dialog header close |
| `CONTROL_MD` | 32 | every toolbar action, every dialog action, icon-only toolbar buttons |
| `CONTROL_LG` | 36 | at most one per screen: the screen's primary action |
| `STATUS_BADGE` | 18 | counts only, never clickable |
| `SAFETY_CHIP` | 20 | safety/content chips, never clickable |
| `TRAY_CHIP` | 28 | tray item container; its × is `CONTROL_SM` |

Retire 20 px buttons. A control is 24, 32 or 36. If a spot feels like it needs 20, it is a
chip, not a button.

### 4.2 Rows and density

| Row | Comfortable | Compact |
|---|---|---|
| Table row | 42 | 38 |
| Sidebar nav item | 30 | 26 |
| Settings row | 44 | 36 |
| Locked-folder row | 52 | 44 |

Density changes rows and vertical gaps, not control heights: a 32 px button stays 32.
`theme::metrics(density)` returns these numbers; screens never read `Density` directly.

### 4.3 Radii

| Token | Value | Use |
|---|---|---|
| `RADIUS_SWATCH` | 3 | category swatch |
| `RADIUS_CONTROL` | 9 | buttons, inputs, nav items, segmented container |
| `RADIUS_INNER` | 12 | review list, callout, warning strip, tray bin |
| `RADIUS_SURFACE` | 14 | cards, summary meter, map well |
| `RADIUS_PILL` | 999 | chips and badges only — never a button |

### 4.4 Spacing

Tokenize the `DESIGN.md` scale and add nothing: 4, 6, 8, 10, 12, 14, 16, 20, 24. Page
padding is `PAGE_TOP = 18`, `PAGE_SIDE = 20`. Gaps between components use 12/16; gaps
inside a component use 4/6/8/10. No new literal spacing values.

### 4.5 Type roles

| Role | Size / weight | Use |
|---|---|---|
| `display(30)`, `display(22)`, `display(18)` | SF Rounded semibold | summary total, done headline, dialog title and tray total |
| `title` | 15 semibold | detail-card name, sheet title |
| `body` | 13 regular/medium | names, labels, dialog body |
| `meta` | 12 | paths, secondary facts, menu labels |
| `caption` | 11.5 | percentages, legend, counts |
| `label` | 11 semibold uppercase | section and column headers |

Remove the 16 px one-off. `ui::text(role, …)` replaces bare `text_size(px(…))` in screens.

### 4.6 Chip vs button invariants

1. A chip or badge is never focusable and never has `on_click`.
2. A pill radius means status, never an action. Locked becomes a `CONTROL_MD` secondary
   button (icon + label, radius 9).
3. Segmented controls are `TabBar::new().segmented()` at one size (outer 32) everywhere:
   Appearance, Density, search scope.
4. One destructive control per surface; neutral/secondary styles never use red or the
   accent fill.

### 4.7 Dialog grammar

| Width class | Value | Use |
|---|---|---|
| `DIALOG_SM` | 480 | result-only sheet, empty state |
| `DIALOG_MD` | 560 | Settings, Locked folders |
| `DIALOG_LG` | 640 | review, and its done state (same width, no jump) |

Footer slots are fixed: `[optional left action] spacer [Cancel or Done] [primary or
destructive]`. Review left action is **Remove all from tray** (v1's "Clear selection" was
wrong: it discards the whole tray and collides with table selection).

## 5. Increments

Rules for every phase: `cargo fmt --all`, `cargo clippy --workspace --all-targets`,
`cargo test --workspace` clean; `sh scripts/bundle_gui.sh` and a light/dark +
comfortable/compact pass before the phase is done. One commit per slice below.

### Phase 0 — Baseline and capture matrix

**Ships:** a script and inventory. No app changes.

- Commit the current working tree as a baseline commit (uncommitted ring map, DESIGN.md
  edits, removed treemap) so later slices are reviewable on their own.
- Add `scripts/gui_captures.sh`: builds the bundle, creates a fixture tree, writes
  `settings.json` (`appearance`, `density`, `window`) into `CLRS_GUI_CONFIG_DIR`, and
  launches with `CLRS_GUI_CAPTURE` + `CLRS_GUI_CAPTURE_DELAY` for each cell of
  2 themes × 2 densities × three window sizes (1280×800, 1440×900, 1920×1080).
- Inventory the controls against §4 and list every call site that does not match.

**Note:** `capture.rs` captures the initial window only. Dialogs are captured after
Phase 4 by hand or from the opt-in E2E lane; do not pretend the script covers them.

**Done when:** captures exist under a git-ignored `target/captures/`, and the inventory
lists each off-grammar control with its file:line.

**Commit:** `chore(gui): capture baseline UI matrix and control inventory`

### Phase 1 — Cleanup safety (blocking, independent of the UI phases)

**Ships:** protected artifacts cannot be trashed from the GUI or the CLI.

Core:

- `crates/core/src/rules.rs`: add `Safety::Protected` (`"protected"` in TOML).
- `crates/core/rules/default.toml`: `ios-simulators` and `android-emulators` become
  `protected`; their descriptions point at `xcrun simctl` / Android Studio's AVD Manager.
- New `crates/core/src/protect.rs`: `pub fn protection(path: &Path) -> Option<&'static str>`
  with reasons for `.git/objects/**`, `.git/lfs/**`, VM disk images (`*.img`, `*.qcow2`,
  `*.vmdk`, `*.vdi`, `*.vhdx`) inside VM stores, and `~/.android/avd/**` disk/snapshot
  files. Conservative: only clearly-internal artifacts; everything else is untouched.
- `crates/core/src/cleanup.rs`: `refuse_unsafe` also calls `protect::protection` and
  returns `Error::Refused { reason }`. This is the choke point for both surfaces.
- `crates/cli/src/commands.rs`: `selects` never selects a `Protected` rule; an explicit
  `--rule <protected-id>` is reported as skipped, not attempted.

GUI:

- `crates/gui/src/selection.rs` / `workbench/mod.rs`: staging a protected path is a no-op
  with a warning; `can_clean` ignores protected entries.
- `explore.rs` (table) and `workbench/explorer.rs` (detail card): show a neutral
  "Protected" chip and disable the add control with a tooltip explaining why.
- `panels.rs` review: list protected entries separately with the reason; the destructive
  button never includes them.
- `recommendations.rs`: protected rule groups are shown but not stageable.

Tests (E2E only):

- `crates/cli/tests/e2e.rs`: fixture HOME with `.git/objects/x.pack`, a custom `--rules`
  file marking a fixture path `protected`, assert `cl-rs clean --safe` and
  `--rule <id>` skip it and exit without trashing it. Use `--dry-run` where applicable.
- `crates/gui/tests/e2e.rs`: stage attempt on the fixture pack file asserts the add
  control is disabled and the Protected label is exposed.

**Done when:** both surfaces refuse, no non-dry-run clean runs in tests, and the review
sheet never lists a protected item as clearable.

**Commit:** `fix(cleanup): protect git internals, VM disks and emulator images`

### Phase 2 — Geometry tokens and the `ui` control API

**Ships:** the grammar exists and the shell uses it. Other screens intentionally still
call buttons directly; Phase 3–4 remove the last direct imports.

- `crates/gui/src/theme.rs`: add the §4 constants and `pub(crate) fn metrics(density)
  -> Metrics` (row, nav, settings row, gaps). Keep `theme.radius = 9` and
  `radius_lg = 14`; stop the kit defaults from being overridden per call site.
- `crates/gui/src/ui.rs`: the only module allowed to construct `Button`.
  - `enum Kind { Primary, Secondary, Ghost, Destructive }`, `enum Size { Small, Regular,
    Large }` mapped to the ladder.
  - `fn action(id, label, size, kind) -> Button` sets label-derived `accessibility_label`.
  - `fn icon_action(id, icon, size, kind)`.
  - `fn segmented(id, options, selected, on_select)` over `TabBar::segmented()`.
  - `fn dialog(width_class)` and `fn footer(left, cancel, primary)`.
  - `fn text(role, …)`, `fn nav_item(...)` (absorbs the local `nav` in `chrome.rs`).
  - `chip`/`badge` stay non-interactive by construction.
- Migrate `crates/gui/src/workbench/chrome.rs`: toolbar (sidebar, map, back/forward,
  breadcrumb, search, Locked, Stop/Refresh) and sidebar (`nav_item`, volume card gear).
  Locked becomes `CONTROL_MD` secondary at radius 9.

**Done when:** the toolbar and sidebar contain no literal heights or radii, and a
screenshot shows Locked reading as a button, not a chip.

**Commit:** `refactor(gui): one control ladder and shared action API`

### Phase 3 — File table, detail card and tray

- `crates/gui/src/explore.rs`: row add and expand-row affordances via `ui::icon_action`
  (24) and `ui::row_action`; keep the full-row click target behavior.
- `crates/gui/src/workbench/explorer.rs`: detail-card actions (Reveal=Secondary,
  Copy=icon, Preview=Secondary, Add to tray=Primary) at one size; results header uses
  `CONTROL_SM` actions and the 32 px segmented scope switch.
- `crates/gui/src/workbench/tray.rs`: 28 px chip container with a 24 px remove button;
  `Review N items` becomes `CONTROL_LG` primary (the one large action on the screen);
  empty tray height follows `metrics()`.
- Every row height comes from `theme::metrics(density)`; no `px(42.)`/`px(38.)` literals
  outside `theme.rs`.

**Done when:** the same "add to tray" action has one size in row, detail and results, and
compact density visibly tightens rows, nav and tray padding.

**Commit:** `refactor(gui): one row, action and tray grammar`

### Phase 4 — Dialogs, segmented controls and footers

- `crates/gui/src/panels.rs`:
  - Review and its done state both use `DIALOG_LG`; Settings and Locked folders use
    `DIALOG_MD`.
  - Footers go through `ui::footer`: review = Remove all from tray (ghost, left) +
    Cancel (secondary) + Move to Trash (destructive); Settings/Locked = Done (primary);
    done state = Open Trash (secondary) + Done (primary).
  - Appearance, Density and the search scope use `ui::segmented`; delete the ghost-button
    rows. Evaluate `setting::SettingGroup`/`SettingItem` for the settings body; if it
    fights the dialog, keep a local row built from `theme` spacing.
  - Locked list scrolls inside a bounded height; Full Disk Access callout stays visible;
    "Open Privacy Settings" becomes a 32 px secondary action, not full-width.
  - Review rows: name, size, classification, consequence; drop the repeated "Your pick"
    label in favour of a single note.
- `crates/gui/src/actions.rs`: menu label `Empty Tray` → `Remove All from Tray`.
- `crates/gui/src/theme.rs`: dialog width classes only (no per-dialog literals).

**Done when:** the four sheets share widths, headers and footer slots; Appearance and
Density render as connected segmented controls, not detached chips.

**Commit:** `refactor(gui): one dialog and segmented-control grammar`

### Phase 5 — Ring map affordances (narrow)

Only what `sunburst.rs` does not already have (it already ships hover, centre readout,
click-to-drill, shared list/map selection and the visibility toggle).

- Tooltip on a wedge: name, full path, exact allocated size, share of the open folder.
- Legend shows only categories present in the inner ring, with labels (not a bare
  green/amber bar); `Safe to clear` / `Worth a look` get a text breakdown.
- Remove duplicated borders/shadows between map well, list card and detail card.
- Not in scope: resizable split, new chart types, animated transitions.

**Done when:** a first-time user can tell what a wedge is without clicking, and colour is
never the only signal.

**Commit:** `feat(gui): map tooltips and readable legend`

### Phase 6 — Keyboard, focus and accessibility

- Every `ui::action`/`ui::icon_action` carries an accessible name from its label or
  tooltip; icon-only controls all get one.
- Focus ring (`ui::focus_outline`) on buttons, segmented segments, nav items and rows;
  tab order: toolbar → sidebar → map → list → detail → tray.
- Escape closes dialogs and returns focus to the trigger; review requires confirmation.
- Re-check contrast for 12 px meta text and 11.5 px captions in both themes; `ink_3` stays
  decorative/disabled only.

**Done when:** the main flows work without a mouse and every control has a visible state
for hover, focus, pressed, disabled and selected.

**Commit:** `a11y(gui): focus order, labels and dialog keyboard flow`

### Phase 7 — Regression gate and enforcement close

- Run the capture matrix and compare before/after at the three window sizes, both themes,
  both densities.
- Delete the remaining direct `gpui_kit::component::button::Button` imports from
  `chrome.rs`, `explorer.rs`, `explore.rs`, `tray.rs`, `panels.rs`; `ui.rs` is the only
  importer. Add `scripts/check_ui_grammar.sh` (grep gate) so a new direct import fails
  review.
- Remove dead primitives and any literal radius/height left outside `theme.rs`.
- Cover: empty state, zero items, denied access, incomplete scan, huge folders, long
  paths, failed Trash operations.

**Done when:** `grep` shows no screen-level button construction or literal control
geometry, and screenshot diffs are reviewed.

**Commit:** `chore(gui): enforce the control grammar and test the matrix`

## 6. Order and dependence

```
Phase 0 ──► Phase 2 ──► Phase 3 ──► Phase 4 ──► Phase 5
                 └────► Phase 6
Phase 1 (independent; land first if review bandwidth allows)
Phase 7 (last)
```

- Phase 1 touches core and can ship before, during or after Phase 0; it blocks nothing.
- Phase 2 must land before 3/4 or those phases recreate the drift.
- Phases 3, 4 and 5 are independent of each other once 2 is in.
- No phase may leave the app half-migrated: if a screen is touched, it is fully moved to
  `ui` in that commit.

## 7. Verification

Per phase:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets   # zero warnings
cargo test --workspace
sh scripts/bundle_gui.sh                # visual pass, light/dark × density
```

Targeted lanes:

```sh
# CLI safety behaviour (Phase 1)
cargo test -p cl-rs --test e2e -- --nocapture

# GUI E2E: opt-in, needs a logged-in macOS session + Accessibility permission
cargo test -p cl-rs-gui --test e2e -- --ignored --nocapture
```

Manual/AX checks per phase: toolbar labels, Locked as a button, one add-to-tray size,
segmented selected state, footer slots, no width jump when review completes.

## 8. Definition of done

- One visual grammar for buttons, chips, badges, segmented controls and dialogs, enforced
  by `ui.rs` being the only constructor.
- Hierarchy per screen: file identity → size → context → one primary action.
- Density visibly changes rows, nav and spacing, not just the table.
- Protected Git/VM/emulator artifacts cannot be staged, trashed or `--rule`-cleaned.
- Light/dark and comfortable/compact stay polished at 1280×800 and above.
- Scan, navigation, cache reuse and cleanup still behave exactly as before.
