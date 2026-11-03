# cl-rs GUI — Design System

**Version:** 0.2.0 (redesign)
**Last updated:** 2026-10-30
**Status:** Approved direction, not yet implemented. See `docs/GUI_REDESIGN_PLAN.md`.
**Reference mock:** https://claude.ai/artifact/FydmoYnXEfXqHiQwU2ai4n (the "Friendly & warm" variant with the treemap is the approved one)

## Direction

cl-rs is a friendly, warm Mac utility that answers one question fast: **what is using my space, and what can I safely get back?**

- **Personality:** friendly and warm, not goofy. Soft surfaces, rounded shapes, an apricot accent and kind, plain copy. It must still feel trustworthy enough to delete things. A mascot may come later and must fit this style. No characters or jokes for now.
- **Structure:** one explorer window. The treemap sits beside the list with its legend underneath, a detail card sits under the list, and a collector tray runs along the bottom. There is no right-hand inspector column.
- **Hero visual:** a one-level squarified treemap coloured by **category**; double-click drills into a folder for detail, and entries too small to label fold into one "smaller items" tile. There is no sunburst.
- **Themes:** light and dark are both first-class. The app follows the system appearance, with an override in Settings.
- **Native first:** use macOS conventions (sheet from the title bar, ⌘ shortcuts, Reveal in Finder, Trash semantics) and the system font for body text.

### Principles
1. **Summary before detail.** Every folder view opens with its size and how much of it is reclaimable.
2. **Colour carries meaning.** Category colours are used everywhere: tiles, row swatches, bars and the legend. Green and amber are reserved for safety. The accent is reserved for actions and selection. Nothing gets a colour just to decorate it.
3. **One place for each thing.** Navigation lives in the sidebar, the current folder in the main area, details in the card, and pending cleanup in the tray. Sidebar sections never hold content to act on.
4. **Visible state.** Staged, safe, review, locked and scanning states each have a visible form (chip, badge, pill or bar), not just a text line.
5. **Safe by construction.** Exploring is read-only. Nothing changes until the user presses the destructive button in the review sheet.

## Tokens

All colours are set in `theme.rs` and mapped onto GPUI Kit `Theme` fields plus a crate-local `Palette` for tokens the kit doesn't have. The values have been checked for WCAG AA contrast (see the table at the end).

### Surfaces and text

| Token | Light | Dark | Use |
|---|---|---|---|
| `window` | `#fffcf9` | `#1e1817` | Main area background |
| `chrome` | `#f7efe8` | `#241d1b` | Title bar/toolbar, sidebar, tray |
| `panel` | `#fbf5ef` | `#221b19` | Treemap well, row hover, summary meter |
| `raised` | `#ffffff` | `#2a2220` | List card, detail card, inputs, tray chips |
| `ink` | `#2e2522` | `#f5ece6` | Primary text |
| `ink_2` | `#7a6a62` | `#b5a69d` | Secondary text, percentages, captions |
| `ink_3` | `#a8988f` | `#7d6f67` | Placeholders, separators, disabled only. Never for content that must be read. |
| `line` | `#eadfd6` | `#3a302c` | Hairlines, card borders, row dividers |

### Accent and states

| Token | Light | Dark | Use |
|---|---|---|---|
| `accent` | `#e2703a` | `#f3925c` | Icons in sidebar, focus ring, selection outline, small marks |
| `accent_fill` | `#b8501d` | `#f3925c` | Filled primary button background |
| `accent_fill_hover` | `#a8461a` | `#f6a578` | Primary button hover |
| `on_accent` | `#ffffff` | `#2a140a` | Text and icons on `accent_fill` |
| `accent_soft` | `#fce6d8` | `#4a2a1a` | Selected row, active sidebar item, icon-button hover |
| `safe` / `safe_soft` | `#276b41` / `#e1f1e5` | `#7ccb98` / `#1f3a29` | "Safe to clear" chip, reclaimable meter |
| `review` / `review_soft` | `#87570c` / `#fbefd6` | `#e8b65c` / `#3d2e12` | "Check first" chip, review meter |
| `danger` | `#b22d3b` | `#ff99a5` | Destructive button, errors |

### Category palette

Each category has one hue, used for treemap tiles, row swatches, size bars and the legend. Tile text is `ink` in light mode. In dark mode tile text is dark (`#1e1817`) on mid-tone tiles.

| Category | Label | Light | Dark | Assigned when |
|---|---|---|---|---|
| `system` | Mac & app data | `#9db4d6` | `#6f8db8` | `~/Library` and its descendants (unless a rule matches), `/System`, `/private` |
| `files` | Your files | `#e8b85c` | `#c99a3e` | Documents, Desktop, Downloads, Pictures, Movies, Music, and media/document file types |
| `cache` | Caches | `#e9906a` | `#d07650` | Rule category `package-caches`, `logs` or `build-output`, or names like `.cache`, `Caches`, `.npm`, `.gradle`, `.cargo`, `.bun` |
| `code` | Code | `#8dbe9a` | `#6fa07c` | A folder whose children include `.git`, or rule category `code-projects` |
| `tools` | Dev tools & models | `#b9a3d3` | `#9580b5` | Rule category `android`, `ios-sdks` or `ai-models`, SDK managers (`.sdkman`, `.rustup`, `.android`), editor folders |
| `apps` | Apps | `#e7a0a8` | `#c27c85` | `.app` packages and `Applications` folders |
| `other` | Other | `#d8ccc2` | `#968a80` | Everything else, plus the "N smaller items" aggregate |

Classification lives in a new GUI module (`category.rs`). It uses only the scanned `Tree` (names, kinds, children) and rule findings, never extra disk reads. A folder inherits its category from the first matching ancestor rule. Otherwise it takes the category of its largest child, which avoids a colourful mess inside `Library`.

### Typography

| Role | Face | Size / weight | Use |
|---|---|---|---|
| Display | SF Pro Rounded (`.AppleSystemUIFontRounded`; fall back to an embedded OFL face such as Figtree if GPUI can't resolve it) | 30 / semibold | Folder size in the summary |
| Title | SF Pro Rounded | 17 / semibold; 15 / semibold | Sheet titles; detail card name |
| Body | System (`.SystemUIFont`) | 13 / regular, 13 / medium for row names | Everything else |
| Caption | System | 11.5 / regular | Percentages, legend, file counts |
| Label | System | 11 / semibold, +0.06 em, uppercase | Sidebar section headers, list column headers |
| Mono | SF Mono (`.AppleSystemUIFontMonospaced`) | 11.5 | Paths and rule commands in the detail card and review sheet |

Every number shown in a column, the summary, the tray or a chip uses tabular figures.

### Spacing, radius, elevation

- **Spacing scale (pt):** 4, 6, 8, 10, 12, 14, 16, 20, 24. Gaps inside a component are 4–10. Gaps between components are 12–16. Page padding is 18 top and 20 sides.
- **Radius:** 14 for cards (list, treemap well, detail, summary meter). 9 for controls (buttons, inputs, nav items, add buttons). 8 for treemap tiles and 5 for sub-tiles. 999 (full) for pills and chips.
- **Borders:** 1 pt `line` on cards. Rows use 1 pt `line` dividers with no zebra stripes.
- **Elevation:** only the detail card and sheets cast a shadow (light: `0 1 2 /6%` + `0 8 24 /7%` warm brown; dark: black at 30%/25%). Everything else is flat on its surface.

## Window layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   ‹ ›  ⌂ Home › Library › Caches          [⌕ Search 3.5M files ⌘F] (🔒 115) ⟳ │ 54  toolbar (chrome)
├───────────────┬──────────────────────────────────────────────────────────────┤
│ PLACES        │ 331.1 GB in Home                ┌ 47.5 GB is safe to clear ┐ │
│ ⌂ Home   331GB│ 3,545,136 files · updated 2s ago│ ███▓▓▓░░░░ + 53 GB to review│ │ summary
│ ▭ Macintosh HD│                                 └──────────────────────────┘ │
│ + Choose…     │ ┌────────── treemap ──────────┐ ┌──────── list ───────────┐ │
│               │ │┌────────┬─────┬───┬──┐     │ │NAME            SIZE      │ │
│ FIND          │ ││Library │Docs │npm│  │     │ │● Library  ▇▇▇▇ 119 GB  + │ │
│ ✦ Suggestions6│ ││ ░░ ░░  │     ├───┤  │     │ │● .npm ⓢ   ▇▇   36.7 GB ✓ │ │
│ ▤ Largest     │ │└────────┴─────┴───┴──┘     │ │● work ⓡ   ▇    23.6 GB + │ │
│ 🔒 Locked  115│ └─────────────────────────────┘ │  …                       │ │
│               │ ● Mac data ● Your files ● …     │                          │ │ legend
│ RECENT        │ ┌ .npm  ⓢ Safe to clear ──────┐ │                          │ │
│ ◷ work/repos  │ │ 36.7 GB · 11% · 149,204 files│ │                          │ │ detail card
│               │ │ npm keeps packages…  [Reveal][⧉][+ Add]│                  │ │
│ ▭ Macintosh HD│ └─────────────────────────────┘ └──────────────────────────┘ │
│ ███░░ 112 GB free                                                            │
├───────────────┴──────────────────────────────────────────────────────────────┤
│ [🗑²]  (npm cache 36.7 GB ×) (pnpm store 10.8 GB ×)        47.5 GB   [Review 2 items] │ 68  tray
└──────────────────────────────────────────────────────────────────────────────┘
```

| Region | Size | Notes |
|---|---|---|
| Window minimum | 720 × 560 | Fits half a laptop screen; hiding the map frees more |
| Toolbar | 54 | Unified with the title bar; traffic lights live in it |
| Sidebar | 210 fixed | `chrome`, 1 pt right hairline |
| Main split | treemap column 0.67fr : list 1fr | Gap 16. The list leads; the treemap keeps proportion context. |
| Tray | 68 with items, 38 empty | Always visible once a scan is loaded; the empty footer is a slim hint that expands when something is staged |

The status bar is removed. Scan source and timing move to the summary subtitle, and staged totals move to the tray.

## Components

### Toolbar
- **Sidebar toggle:** icon-only, far left, `panel-left-close` / `panel-left-open`, tooltip "Hide sidebar (⌥⌘S)". The choice is persisted.
- Icon-only **Back** and **Forward** buttons (`chevron-left` / `chevron-right`, 32×32) with tooltips and accessible labels "Back" and "Forward". Disabled ones drop to 35% opacity.
- **Breadcrumb:** a home icon, then each ancestor as a ghost button, separated by `›` in `ink_3`. Collapse the middle to `…` (a menu of the hidden ancestors) when it doesn't fit. Clicking an ancestor navigates. Up (⌘↑) stays as a keyboard action.
- **Search field:** 280 wide, 32 high, `raised`, search icon, placeholder "Search files and folders", ⌘F. A scope switch ("This folder" / "Everywhere") appears only as a segmented control on the results header, never in the toolbar.
- **Locked pill:** shown only when there are scan issues. 32 high, lock icon plus "115 locked". It opens the Locked folders sheet. It replaces the full-width grey banner.
- **Rescan** icon button (`refresh-cw`), 32 high. While scanning it becomes a **Stop** button with a spinner.
- There is no title text ("Explore"). The sidebar toggle is the only view toggle; the treemap is always visible.

### Sidebar
Sections have uppercase labels: **Places**, **Find**, **Recent**.
- **Nav item:** height 30, radius 9, 16 pt icon in `accent`, label in `ink` 13/medium, and an optional right-aligned meta value (size or count) in `ink_2`. Hover is `ink` at 4%. Active uses `accent_soft` with a 650 weight label. Items are left-aligned (never centred).
- **Places:** Home (with its size), the startup volume (with its total), Choose folder… (⌘O).
- **Find:** Suggestions (count badge in `accent_fill`), Largest files, Locked folders (count). Each one opens a result list in the main area. Suggestions are no longer listed one by one in the sidebar.
- **Recent:** up to 5 recent roots with their last known size.
- **Footer:** a volume meter card: volume name, a 7 pt bar (used/free) and "112 GB free of 994 GB". Settings moves to the app menu (⌘,) and a gear in the card's corner.

### Summary row
- Left: the display-size total ("331.1 GB" plus "in Home" at 16/semibold `ink_2`), with a subtitle line: file count · scan source and age ("updated 2 seconds ago from your last scan" or "scanned in 14.5 s").
- Right: the **Reclaimable** card (`panel`, radius 14, width 380): "47.5 GB is safe to clear" (`safe`, title size) and "+ 53 GB worth a look" in `ink_2`, over an 8 pt stacked meter (safe then review, as a share of the folder). Clicking it opens the Suggestions list.
- When a folder has no suggestions, the card reads "Nothing to clear here" in `ink_2` with an empty meter.

### Treemap
- It sits in a `panel` well (radius 14, 8 pt inset) that fills the column height left over after the legend. It is always visible. The toggle is removed.
- **Tiles:** a 3 pt gutter between tiles, radius 8, category fill, and a label (name 12.5/semibold plus size 11.5 at 75% opacity) when the rendered tile is at least 48×28. Tiles too small to label fold into the `+N` tail.
- **Nesting:** tiles at least 110×110 draw up to 8 sub-tiles of their own children underneath the label, offset 44 pt from the top. Children below 4% of the parent fold into one `+N` sub-tile. Sub-tiles are 5 pt-radius blocks in white at 28% (light) or black at 16% (dark), labelled when at least 56×22. Clicking a sub-tile selects that child. Double-clicking enters it.
- **Suggestion marker:** an 18 pt circle in the tile corner, with a shield-check (`safe`) or an eye (`review`).
- **States:** hover brightens the tile by 7% and highlights the matching list row. Selection draws a 3 pt `ink` inner outline. The list and the treemap share selection and hover.
- **Tail:** everything beyond 80 tiles, or below 0.5% of the area, becomes one `+N` tile in `other`. Tiles that cannot fit a label fold into it too, so every named tile is readable.
- **Legend:** one row under the well, showing only categories present in the current folder: a 10 pt swatch (radius 3) plus the label in caption `ink_2`.

### List
- A card (`raised`, radius 14, 1 pt `line`) with a sticky header: NAME · (bar) · SIZE · (action).
- **Row:** height 38 comfortable / 30 compact. Columns are:
  1. a 10 pt category swatch, 16 pt folder/file icon in `ink_2`, the name (13/medium, truncated) and a safety chip if a rule matched;
  2. a 6 pt size bar, relative to the largest sibling, in the category colour;
  3. the size (semibold, right-aligned) with its percentage of the folder below in caption;
  4. a 24 pt **add** button (`plus`; becomes `check` on `accent_fill` when staged).
- File counts and Kind move to the detail card.
- Hover uses `panel`. Selection uses `accent_soft`. Rows are separated by 1 pt dividers with no zebra stripes.
- **Results mode** (search, Largest files, Suggestions, Locked folders): a header bar inside the card shows the title, a count ("largest 500 of 992"), a scope segmented control for search, **Add all**, and a close button (×). Names show the path relative to the root, with the parent path in `ink_2` and the leaf in `ink`.

### Chips and badges
- **Safety chip:** a 20 pt pill, 11/bold. "Safe to clear" uses `safe` on `safe_soft` with a check icon. "Check first" uses `review` on `review_soft`. Rows shorten a contained match to the rule name ("Gradle caches") in an outlined pill; the full "Contains Gradle caches" sentence lives in the tooltip and the accessible label.
- **Count badge:** 18 pt pill on `accent_fill` / `on_accent`, 11/bold.

### Detail card
- Under the list, `raised`, radius 14, shadow. With a selection it shows the item. With nothing selected it collapses to one line: the category swatch, folder name, path and folder actions, so the header's size and file count are not repeated.
- **Line 1:** the category swatch, the name (title, 15/semibold), its safety chip, then the actions on the right.
- **Line 2:** facts in `ink_2`: **size** (in `ink`) · % of the parent · files · category · kind.
- **Line 3:** if a rule matched, the rule's plain-language description (from a new `description` field in the rules TOML). Otherwise the full path in mono `ink_3`, which is selectable.
- **Actions** sit at the end of the first line: **Reveal** (secondary), **Copy path** (icon), and **Add to tray** (primary), which becomes **In tray** (secondary, with a check).
- With several items selected: "3 items · 52.6 GB" and the actions apply to all of them.

### Collector tray
- `chrome` background with a 1 pt top hairline, height 68.
- **Left:** a 42 pt trash well (`raised`, radius 12) with an `accent_fill` count badge. When something is added, the well gives a single 0.4 s scale/tilt bump (disabled under Reduce Motion).
- **Middle:** staged items as chips (`raised` pill: name, size in `ink_2`, × remove). The chips scroll horizontally and the last visible one reads "+4 more" when they overflow. Rule-matched items show the rule name ("npm cache"), not the folder name.
- **Empty state:** "Nothing set aside yet. Use + on anything you'd like to clear, then review it here."
- **Right:** the total (title size) with "to clear" beneath, then the **Review N items** primary button (36 high), which is disabled when the tray is empty.

### Review sheet
- A native sheet from the title bar, 560 wide.
- **Title:** "Clear 47.5 GB?" **Body:** "These go to the Trash, so you can still put them back. Tools with their own cleanup command run that instead."
- **Item list** (bordered, radius 12): the name plus safety chip, a description, the mono action line (`Runs npm cache clean --force` or `Move ~/.android/avd to Trash`) and the size on the right.
- **Footer:** an info note (amber text if any item is "Check first"), then **Cancel** and **Move to Trash** (`danger` fill, trash icon). There is no separate "Done" button.
- **While cleaning:** the destructive button shows a spinner with "Clearing 2 of 3…".
- **Done state** (same sheet): a 120 pt `safe` progress ring with a check (it fills over 1.2 s; static under Reduce Motion), "47.5 GB is on its way out", "Empty the Trash to get the space back.", then **Done** and **Open Trash**. Failures list below in `danger` and stay in the tray.

### Locked folders sheet (replaces Scan Issues)
- **Title:** "115 folders couldn't be read". **Subtitle:** "Their contents aren't counted in the totals."
- When Full Disk Access is the cause: a callout card with three numbered steps (System Settings → Privacy & Security → Full Disk Access → add cl-rs) and **Open Privacy Settings**. The numbers are justified because these are real sequential steps.
- **List:** a lock icon, the path in mono, and a humanised reason ("macOS blocked access" for EPERM/EACCES, "Removed during scan" for ENOENT) in place of `Operation not permitted (os error 1)`. Consistent full-width dividers.

### Scanning state
- **First scan:** the main area shows a centred card with a spinner ring, "Looking through Home…", live counters (files · folders · size found, tabular) and **Stop**.
- **Rescan with an existing result:** the old view stays. A thin 2 pt indeterminate bar appears under the toolbar, and the summary subtitle reads "Rescanning… 1.2M files so far".

### Empty state (first launch)
- Centred in the main area: a simple illustration slot (reserved for the future mascot; for now, a warm-tinted hard-drive glyph), then "See what's using your space" (title 22), one line of body text, **Scan Home** (primary) and **Choose folder…** (secondary), and the reassurance line "Looking is read-only. Nothing moves until you review it."
- The tray and the summary are hidden until a scan exists.

### Settings sheet
- Grouped rows with labels on the left and controls on the right: Appearance (System / Light / Dark segmented), List density (Comfortable / Compact segmented), Reuse saved scans (switch), Scan other volumes (switch). A footer note explains what each does.

## Voice and copy

- Speak from the user's side: "clear", "set aside", "locked folders", "your files". Avoid "staged", "allocated", "unreadable paths" and "os error 1" in the UI. "Allocated" stays only in the size tooltip ("119.3 GB on disk (allocated blocks)").
- Be kind and specific: "npm keeps packages it downloaded once. It fetches them again when a project needs them."
- Buttons say exactly what happens: "Move to Trash", "Add to tray", "Review 2 items".
- No exclamation marks and no jokes for now. Warmth comes from plain wording, not cuteness.

## Motion

- Hover and selection: 120 ms ease-out colour/opacity changes.
- Tray bump: 400 ms on add. Sheet: native. Done ring: 1.2 s fill.
- Everything respects Reduce Motion (`NSWorkspace.accessibilityDisplayShouldReduceMotion`): no bump, a static ring, no transitions.

## Icons

Lucide, through `gpui_kit_assets::icon_assets!`. Besides the defaults, the app needs: `house`, `hard-drive`, `folder-plus`, `sparkles`, `lock`, `clock`, `trash-2`, `layout-grid`, `scan-search`, `package`, `code`, `film`. Icons are 16 pt at stroke 2 and never emoji.

## Accessibility

- Every icon-only control has an `aria_label` matching its tooltip ("Back", "Forward", "Rescan", "Add Library to tray").
- Each treemap tile is a `Role::ListItem` labelled "Library, 119.3 GB, 36% of Home, Mac & app data" (plus ", safe to clear" when a rule matched).
- Safety and category are never conveyed by colour alone: there is always a chip text, a legend label or an accessible label.
- Focus ring: 2 pt `accent` with a 2 pt offset. Tab order: toolbar → sidebar → treemap → list → detail card → tray.
- The tray uses `Role::Status`, so totals are announced when they change.

### Contrast (WCAG 2.1)

| Pair | Light | Dark | Target |
|---|---|---|---|
| `ink` on `window` | 14.6 | 15.0 | 4.5 |
| `ink_2` on `window` / `chrome` | 5.1 / 4.5 | 7.4 | 4.5 |
| `on_accent` on `accent_fill` | 5.0 | 7.6 | 4.5 |
| `safe` on `safe_soft` | 5.5 | 6.4 | 4.5 |
| `review` on `review_soft` | 5.4 | 7.1 | 4.5 |
| Tile text on the lowest-contrast category | 6.2 (cache) | 5.0 (tools) | 4.5 |

`ink_3` (2.7 light / 3.6 dark) is decorative or disabled only.

## Keyboard

| Shortcut | Action |
|---|---|
| ⌘O | Choose folder |
| ⌘R / ⌘⇧R | Rescan (reuse saved scan) / fresh scan |
| ⌘. | Stop scan |
| ⌘[ / ⌘] | Back / Forward |
| ⌘↑ / ⌘↓ or Return | Parent folder / open selected |
| ⌘F, Esc | Search; close results |
| ⌘L | Largest files |
| ⌘T / ⌘⇧T | Add selection to tray / remove from tray |
| ⌘⌫ | Add to tray and open review |
| ⌘⇧C | Copy path |
| ⌥⌘S | Show/hide sidebar |
| ⌘, | Settings |

⌘I (inspector) is removed.

## Persistence

Unchanged: `~/Library/Application Support/cl-rs/settings.json` (appearance, density, window bounds, cache, cross-devices, recent roots), written atomically. The inspector flag is dropped. Sidebar visibility is stored alongside them.

## Anti-patterns

- No colour without meaning (no rank-cycled pastel hues).
- No text-only buttons that look like labels, and no Show/Hide toggle for the treemap; the sidebar toggle is the one view control.
- No full-width grey banners for status. Use pills, chips or a callout inside the relevant sheet.
- No raw error strings or byte counts in the main UI.
- No centred sidebar items, zebra rows, or empty table columns.
- No emoji icons, gradients, glassmorphism, or a shadow on every card.
- No sunburst or second chart type.
