# GUI control inventory (Phase 0 baseline)

Captured 2026-11-05 against the ring-map explorer. This is the pre-grammar baseline:
every control below is compared with §4 of `disk-analyzer-ui-improvement-plan.md`.

Regenerate the screenshots with:

```sh
sh scripts/gui_captures.sh
```

Outputs land in `target/captures/` (git-ignored): 16 cells of
`welcome|explorer` × `Light|Dark` × `Comfortable|Compact` × `1280x800|1440x900`.
`1920x1080` is skipped on this 1512×982 display; the script records skips in
`manifest.tsv`. PNGs are Retina-sized (2× the logical window).

## Verified baseline measurements

| What | Value | How |
|---|---|---|
| Table row pitch, comfortable | 84 px = **42 pt** | divider scan of the capture |
| Table row pitch, compact | 76 px = **38 pt** | same |
| Table header | 68 px = **34 pt** | same |
| Dialog widths | 580 / 480 / 580 / 480 pt | `panels.rs` |
| Toolbar / sidebar / tray | 54 / 210 / 68 pt | `chrome.rs`, `theme.rs`, `tray.rs` |

Density is honoured for table rows only. Every other surface is identical between the
comfortable and compact captures, so `Density` currently affects nothing outside the
table.

## Buttons

`md` = 32 pt default, `sm` = 24 pt, `xs` = 20 pt. Radius is `theme.radius` (9 pt) unless
noted. Target sizes are from §4.1.

| Call site | Current | Target |
|---|---|---|
| `chrome.rs:30` sidebar toggle | icon ghost md | icon md |
| `chrome.rs:48` map toggle | icon ghost md | icon md |
| `chrome.rs:67` back | icon ghost md | icon md |
| `chrome.rs:74` forward | icon ghost md | icon md |
| `chrome.rs:97` Locked | outline md, **rounded 999** | secondary md, radius 9 |
| `chrome.rs:112` Stop | outline md | secondary md |
| `chrome.rs:122` Refresh/Retry | icon ghost md | icon md |
| `chrome.rs:189` breadcrumb crumb | ghost md | ghost md |
| `chrome.rs:386` Settings gear | icon ghost **sm** | icon sm |
| `tray.rs:107` remove staged item | icon ghost **xs**, **rounded 999** | icon sm, radius 9 |
| `tray.rs:128` Review N items | primary md | **primary lg (36)** |
| `panels.rs:222` "Empty tray" | ghost md | ghost md, rename **Remove all from tray** |
| `panels.rs:233` Cancel | outline md | secondary md |
| `panels.rs:239` Move to Trash | danger md | destructive md |
| `panels.rs:331` Open Trash | outline md | secondary md |
| `panels.rs:346` Done (result) | primary md | primary md |
| `panels.rs:401` Open Privacy Settings | outline **sm** | secondary md |
| `panels.rs:467` Done (locked) | primary md | primary md |
| `panels.rs:518` Appearance | **sm ghost** buttons | segmented |
| `panels.rs:541` List density | **sm ghost** buttons | segmented |
| `panels.rs:600` Done (settings) | primary md | primary md |
| `explorer.rs:111` dismiss warning | icon ghost **xs** | icon sm |
| `explorer.rs:154` Scan Home | primary md | **primary lg** |
| `explorer.rs:161` Choose folder | outline md | secondary md |
| `explorer.rs:241` Stop (welcome scan) | outline **sm** | secondary md |
| `explorer.rs:891` Reveal | outline **sm** | secondary md |
| `explorer.rs:901` Copy path | icon outline **sm** | icon md |
| `explorer.rs:911` Preview | outline **sm** | secondary md |
| `explorer.rs:1043` Add to tray / In tray | **sm**, primary/outline | primary/secondary md |
| `explorer.rs:1208` search scope | **xs ghost** buttons | segmented |
| `explorer.rs:1223` Add all | **xs** outline | secondary sm |
| `explorer.rs:1231` close results | icon **xs** ghost | icon sm |
| `explore.rs:597` row add / added | icon **sm** ghost/primary | icon sm |

Same action, three sizes today: row add (sm), detail Add to tray (sm), Add all (xs),
Review (md). Icon-only actions: md toolbar, sm gear, xs dismiss/remove/close.

## Chips and badges

| Call site | Current | Verdict |
|---|---|---|
| `ui.rs:84` content chip | 20 pt pill, non-interactive | compliant |
| `ui.rs:118` safety chip | 20 pt pill, non-interactive | compliant |
| `chrome.rs:450` sidebar count badge | 18 pt pill | compliant (`STATUS_BADGE`) |
| `tray.rs:88` tray item | 28 pt pill container | compliant (`TRAY_CHIP`) |
| `chrome.rs:97` Locked | 32 pt **button** at 999 | violates "pill = status" |

## Dialogs

| Sheet | Current | Target |
|---|---|---|
| Review | 580 | `DIALOG_LG` 640 |
| Review done state | 480 | `DIALOG_LG` 640 |
| Locked folders | 580 | `DIALOG_MD` 560 |
| Settings | 480 | `DIALOG_MD` 560 |

Footers use three grammars: review = ghost left + outline + danger; Settings and Locked =
single primary Done; done state = outline + primary.

## Typography and geometry literals

| Value | Call site | Role |
|---|---|---|
| 16 pt | `explorer.rs:389` | one-off; remove (title 15 or meta 12) |
| 15 pt | `explorer.rs:939` | `title` |
| 13 pt | `workbench/mod.rs:1062` | `body` |
| 12 pt × 15 | several | `meta` |
| 11.5 pt × 8 | several | `caption` |
| 11 pt × 8 | several | `label` |

Heights and radii outside `theme.rs`: `ui.rs:51` (6), `ui.rs:84/118` (20),
`tray.rs:28` (38), `tray.rs:69` (18), `tray.rs:88` (28), `tray.rs:137` (68),
`chrome.rs:132` (54), `chrome.rs:399` (7), `chrome.rs:450` (18), `chrome.rs:498` (30),
`explorer.rs:316` (8), `panels.rs:413` (260), `panels.rs:431` (52), review rows 76/88.
Radii: 3, 9, 10, 12, 14, 999.

## Confirmed defects found while capturing

1. **Locked is a chip-shaped button.** `chrome.rs:97` is a 32 pt `Button` with
   `rounded(px(999))`, the same silhouette as the 20 pt safety chip. Visible in every
   `explorer-*` capture once the fixture contains an unreadable folder (`1 locked`).
2. **"Segmented" controls are detached buttons.** Appearance/Density are 24 pt ghost
   buttons (`panels.rs:518`, `541`); search scope is 20 pt (`explorer.rs:1208`).
   gpui-component already ships `TabBar::new().segmented()`.
3. **"1 folders unread".** `explorer.rs:275` always says `folders`, unlike every other
   count that goes through `format::plural`.
4. **Density stops at the table.** Verified by pixel measurement: rows change 42 → 38 pt,
   nav, settings rows, tray and gaps do not change at all.

## Already compliant

- Colour tokens and both themes (`theme.rs`); chips carry meaning without colour alone.
- Card radius 14, control radius 9, toolbar 54, sidebar 210, tray 68/38, nav item 30.
- Row size bar (6) and summary meter (8) are decoration, not controls.
- `ui::focus_outline` exists and is used by sidebar nav items.
