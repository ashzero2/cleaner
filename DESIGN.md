# cl-rs GUI — Design System

**Version:** 0.1.0  
**Last Updated:** 2026-10-28  
**Status:** Phase 1 Complete (Read-Only Disk Exploration)

## Overview

cl-rs is a native macOS disk space analyzer with a restrained, professional workbench interface. The GUI calls the `clrs-core` library directly, reusing the scanner and cache without duplicate logic or destructive actions.

**Design mode:** Operate — the visitor completes a task (explore disk usage, inspect folders, navigate directories). Scanability, consistency, and native expectations outrank expression.

## Token Set

### Colors

#### Light Theme

| Token | Value | Usage |
|-------|-------|-------|
| `background` | `#ffffff` | Main window area |
| `foreground` | `#20252c` | Primary text |
| `muted` | `#f4f5f7` | Scan progress, disabled surfaces |
| `mutedForeground` | `#59616c` | Secondary text, labels |
| `border` | `#d8dde4` | Dividers, borders |
| `primary` | `#006c75` | Primary buttons, selected states |
| `primaryHover` | `#005a63` | Primary button hover |
| `primaryActive` | `#004b53` | Primary button active |
| `primaryForeground` | `#ffffff` | Text on primary buttons |
| `selection` | `#dceff0` | Table row selection, button selected bg |
| `sidebar` | `#eceef1` | Sidebar background |
| `sidebarForeground` | `#20252c` | Sidebar text |
| `sidebarBorder` | `#d8dde4` | Sidebar divider |
| `tableActive` | `#dceff0` | Active table row |
| `tableActiveBorder` | `#006c75` | Active row indicator |
| `tableHead` | `#f4f5f7` | Table header background |
| `tableHeadForeground` | `#59616c` | Table header text |
| `tableHover` | `#f4f5f7` | Table row hover |
| `tableEven` | `#ffffff` | Even row background |
| `tableRowBorder` | `#d8dde4` | Row dividers |
| `danger` | `#b22d3b` | Error states, warnings |
| `warning` | `#875400` | Warning states |

#### Dark Theme

| Token | Value | Usage |
|-------|-------|-------|
| `background` | `#22252a` | Main window area |
| `foreground` | `#f1f3f5` | Primary text |
| `muted` | `#191b1f` | Scan progress, disabled surfaces |
| `mutedForeground` | `#adb5c0` | Secondary text, labels |
| `border` | `#414751` | Dividers, borders |
| `primary` | `#68cfd6` | Primary buttons, selected states |
| `primaryHover` | `#8adbe0` | Primary button hover |
| `primaryActive` | `#4db9c1` | Primary button active |
| `primaryForeground` | `#12282b` | Text on primary buttons |
| `selection` | `#183e43` | Table row selection, button selected bg |
| `sidebar` | `#191b1f` | Sidebar background |
| `sidebarForeground` | `#f1f3f5` | Sidebar text |
| `sidebarBorder` | `#414751` | Sidebar divider |
| `tableActive` | `#183e43` | Active table row |
| `tableActiveBorder` | `#68cfd6` | Active row indicator |
| `tableHead` | `#191b1f` | Table header background |
| `tableHeadForeground` | `#adb5c0` | Table header text |
| `tableHover` | `#191b1f` | Table row hover |
| `tableEven` | `#22252a` | Even row background |
| `tableRowBorder` | `#414751` | Row dividers |
| `danger` | `#ff99a5` | Error states, warnings |
| `warning` | `#f0c36a` | Warning states |

### Typography

- **Font family:** `.SystemUIFont` (system default sans-serif)
- **Body size:** `13pt`
- **Heading scale:** `text_2xl` (approx. 22pt), `font_semibold`
- **Tracking floor:** `-0.04em` (default system behavior)
- **Line heights:** 1.25 (buttons), 1.4 (body), 1.3 (headings)

### Spacing

| Token | Value |
|-------|-------|
| `gap_1` | `4px` |
| `gap_2` | `8px` |
| `gap_3` | `12px` |
| `gap_4` | `16px` |
| `gap_5` | `20px` |
| `gap_6` | `24px` |
| `px_2` | `8px` |
| `px_3` | `12px` |
| `px_4` | `16px` |
| `px_5` | `20px` |

### Radii

| Token | Value | Usage |
|-------|-------|-------|
| `radius` | `6px` | Buttons, sheet headers |
| `radius_lg` | `10px` | Dialogs, elevated surfaces |

### Densities

- **Comfortable:** Default, medium row height, 8px padding
- **Compact:** Reduced row height, 4px padding

### Dimensions

| Token | Value | Usage |
|-------|-------|-------|
| `sidebarWidth` | `200pt` | Sidebar width |
| `inspectorWidth` | `280pt` | Inspector width |
| `minWidth` | `1040pt` | Minimum window width |
| `minHeight` | `680pt` | Minimum window height |
| `toolbarHeight` | `56pt` | Top toolbar |
| `statusBarHeight` | `30pt` | Bottom status bar |
| `sheetMarginTop` | `48pt` | Sheet top inset (macOS native) |

## Component Library

### Button

**Variants:**
- **Primary** (`button_primary`): Teal background, white text — primary actions
- **Secondary** (`button_secondary`): Muted background, foreground text — secondary actions
- **Ghost** (`bg_transparent`): No background, foreground text — inline actions

**States:**
- Default: `primary` or `secondary` token colors
- Hover: `button_primary_hover` or `button_secondary_hover`
- Active: `button_primary_active` or `button_secondary_active`
- Selected: Background `selection`, border `primary`, text `foreground`
- Disabled: Muted colors, reduced opacity

**API:**
```rust
Button::new(id)
    .label("Action")
    .primary()
    .secondary()
    .ghost()
    .selected(true)
    .w_full()
    .on_click(listener)
```

### Switch

**States:**
- **Off:** Track `switch.background` (muted), thumb `switch.thumb`
- **On:** Track `primary`, thumb offset right

**Usage:**
```rust
Switch::new("id")
    .label("Label text")
    .checked(true)
    .on_change(listener)
```

### Table

**Columns:**
- **Name:** 280pt width, 100pt min, icon + text
- **Allocated:** 120pt width, 80pt min, right-aligned
- **Files:** 90pt width, 70pt min, right-aligned
- **Kind:** 90pt width, 60pt min, right-aligned

**States:**
- Default row: `table_even`
- Hover: `table_hover`
- Selected: `table_active` with `table_active_border` indicator
- Active column: `table_active` background

**API:**
```rust
Column::new("id", "Label")
    .width(px(280.))
    .min_width(px(100.))
    .text_right()
    .resizable(true)
```

### Sheet

**Usage:**
- **Settings:** 440pt width, grouped controls with tight headings
- **Issues:** 540pt width, scrollable list of unreadable paths

**Layout:**
```rust
sheet
    .title("Title")
    .size(px(440.))
    .child(v_flex().gap_6()
        .child(v_flex().gap_2()
            .child(div().font_semibold().child("Heading"))
            .child(control_group)
        )
    )
    .footer(Button::new("done").label("Done").primary())
```

## Layout Patterns

### Workbench

**Structure:**
```
┌─────────────────────────────────────────────────────────┐
│ Toolbar (56pt)                                          │
├──────────┬──────────────────────────────┬───────────────┤
│ Sidebar  │ Content Area                 │ Inspector     │
│ (200pt)  │                              │ (280pt)       │
│          │ ┌──────────────────────────┐ │               │
│ Locations│ │ Breadcrumb Bar (44pt)    │ │ Details       │
│          │ ├──────────────────────────┤ │               │
│ Recent   │ │ Virtualized Table        │ │               │
│          │ │                          │ │               │
│ Settings │ └──────────────────────────┘ │               │
└──────────┴──────────────────────────────┴───────────────┘
│ Status Bar (30pt)                                         │
└─────────────────────────────────────────────────────────┘
```

**Rules:**
- Sidebar items left-aligned, current root marked with `selected` state
- Inspector hidden by default, toggled via Cmd-I
- Table virtualized, largest-first ordering
- Empty state shows welcome screen with two-action primary/secondary pattern

### Empty State (Welcome)

**Copy:**
- Heading: `"See what's using your space."`
- Subheading: `"Choose a folder to explore its disk usage. Start with Home, or look at a specific project or drive."`
- Actions: `"Scan Home"` (primary), `"Choose Folder…"` (secondary)
- Footer: `"Read-only exploration. Nothing is moved or deleted."`

### Settings Sheet

**Groups:**
1. **Appearance:** System / Light / Dark buttons
2. **List density:** Comfortable / Compact buttons
3. **Scanning:**
   - Switch: "Reuse saved scans"
   - Switch: "Scan across mounted volumes"
   - Note: "Scan settings apply to the next scan. Cross-volume scans do not use saved scans. Symlinks are never followed."
4. **Footer:** "cl-rs only reads your files. It never moves or deletes anything."

### Issues Sheet

**Copy:**
- Count header: `"{count} paths could not be read. Their contents are excluded from the totals."`
- FDA guidance (if needed): "macOS denied access to protected folders. In System Settings → Privacy & Security → Full Disk Access, add the packaged cl-rs app. Quit and reopen it, then refresh the scan."
- Action: "Open Privacy Settings" button

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| **Cmd-O** | Choose folder |
| **Cmd-R** | Refresh (reuse cache) |
| **Cmd-Shift-R** | Fresh scan |
| **Cmd-.** | Stop scan |
| **Cmd-Up** | Parent folder |
| **Cmd-Down** | Open selected folder |
| **Cmd-I** | Toggle inspector |
| **Cmd-Shift-C** | Copy path |
| **Cmd-,** | Open settings |
| **Cmd-Q** | Quit |
| **Up / Down** | Select table rows |
| **Tab** | Focus traversal |

## Accessibility

- **Role assignments:**
  - Workbench root: `Role::Group`, `AXRole::AXGroup`, label "Disk space workbench"
  - Table rows: `Role::Row`, `AXRole::AXTableRow`
  - Table cells: `Role::Cell`, `AXRole::AXStatic`, with `cell_label` method for screen readers
  - Status bar: `Role::Status`, `AXRole::AXStatusItem`
- **Keyboard focus:** All interactive elements focusable via Tab
- **Screen reader:** Full labels via `accessibility_label`, table uses `cell_label` for row data

## Dark Mode

- **Default:** Follows system appearance (`Appearance::System`)
- **Override:** User can select Light or Dark explicitly in Settings
- **Transition:** Immediate toggle, no animation (native system behavior)
- **Tokens:** All colors defined in both light and dark palettes, no automatic flip

## State Management

- **Selection:** `selected: Option<NodeId>` — cleared on navigation, preserved across scan updates
- **Loading:** `running: Option<Running>` — progress, started time
- **Failed:** `failed: bool` — retry state for failed roots
- **Warning:** `warning: Option<String>` — transient errors (cache load failures, settings parse errors)
- **Inspector:** `inspector: bool` — toggle visibility

## Persistence

**Preferences path:** `~/Library/Application Support/cl-rs/settings.json`

**Serialized fields:**
- `appearance`: System / Light / Dark
- `density`: Comfortable / Compact
- `window`: x, y, width, height (sane bounds only)
- `cache_enabled`: bool
- `cross_devices`: bool
- `recent`: Array of max 10 PathBuf (non-UTF8 excluded)

**Write pattern:** Atomic (temp file, fsync, rename, mode 0600)

**Load pattern:** 64 KiB bounded, defaults on corrupt/unusable

## Color Contrast Validation

| Context | Light | Dark | Target | Result |
|---------|-------|------|--------|--------|
| Body on background | 15.41:1 | 13.82:1 | 4.5:1 | PASS |
| Secondary on background | 6.27:1 | 7.43:1 | 4.5:1 | PASS |
| Sidebar on background | 5.39:1 | 8.33:1 | 3:1 | PASS |
| Primary button text | 6.17:1 | 8.42:1 | 4.5:1 | PASS |
| Error text | 6.30:1 | 7.58:1 | 4.5:1 | PASS |

## Anti-Patterns Avoided

- ❌ Hero metrics / dashboard cards
- ❌ Treemap visualization
- ❌ Recommendations / "review queue"
- ❌ Gradient text or backgrounds
- ❌ Glassmorphism / blur decoration
- ❌ Search functionality (Phase 1)
- ❌ Cleanup actions / Trash execution
- ❌ Dual-axis charts
- ❌ Rainbow palettes
- ❌ Emoji as icons

## Implementation Notes

**GPUI Stack:**
- `gpui-kit = "=0.8.1"`
- `gpui-pre = 0.3.10`
- `gpui-pre-macos = 0.3.10` (vendor patch for stable Rust)

**Architecture:**
- `jobs.rs`: Background worker with `Arc<Progress>` and generation tracking
- `workbench.rs`: UI state, accepts only current-generation results
- `explore.rs`: Virtualized table delegate, O(1) row lookup
- `format.rs`: Number, allocation, path formatting
- `theme.rs`: Token application, theme switching

**No comments restate the code.** Doc comments only on public items whose name isn't self-explanatory.
