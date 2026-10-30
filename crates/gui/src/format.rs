use std::ffi::OsStr;
use std::path::Path;

use clrs_core::EntryKind;

/// Decimal (1000-based) allocated size, matching Finder and the CLI.
pub(crate) fn allocated(value: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut unit = 0;
    let mut scaled = value;
    while scaled >= 1000 && unit < UNITS.len() - 1 {
        scaled /= 1000;
        unit += 1;
    }
    if unit == 0 {
        return format!("{value} B");
    }
    let divisor = 1000u64.pow(u32::try_from(unit).unwrap_or(0));
    let whole = value / divisor;
    let tenths = value % divisor * 10 / divisor;
    format!("{whole}.{tenths} {}", UNITS[unit])
}

pub(crate) fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Display form of a path: the home directory becomes `~`; invalid UTF-8 is replaced.
pub(crate) fn path(path: &Path) -> String {
    if let Some(home) = std::env::home_dir()
        && !home.as_os_str().is_empty()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", rest.to_string_lossy())
        };
    }
    path.to_string_lossy().into_owned()
}

/// A folder's name for display; the home folder is "Home".
pub(crate) fn name(path: &Path) -> String {
    if std::env::home_dir().is_some_and(|home| home == path) {
        return "Home".to_owned();
    }
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Display kind; folders named like macOS bundles are packages.
pub(crate) fn kind(kind: EntryKind, name: &OsStr) -> &'static str {
    match kind {
        EntryKind::File => "File",
        EntryKind::Dir if is_package(name) => "Package",
        EntryKind::Dir => "Folder",
        EntryKind::Symlink => "Symlink",
        EntryKind::Other => "Other",
    }
}

fn is_package(name: &OsStr) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        ["app", "framework", "bundle", "plugin", "kext", "xpc"]
            .iter()
            .any(|package| extension.eq_ignore_ascii_case(package))
    })
}

/// A share as a percentage with one decimal; tiny non-zero shares read "<0.1%".
pub(crate) fn percent(fraction: f64) -> String {
    let percent = fraction * 100.;
    if percent > 0. && percent < 0.1 {
        "<0.1%".to_owned()
    } else {
        format!("{percent:.1}%")
    }
}

/// How long ago `elapsed` was, in words.
pub(crate) fn ago(elapsed: std::time::Duration) -> String {
    match elapsed.as_secs() {
        0..=9 => "just now".to_owned(),
        seconds @ 10..=59 => format!("{seconds} seconds ago"),
        seconds @ 60..=3599 => match seconds / 60 {
            1 => "a minute ago".to_owned(),
            minutes => format!("{minutes} minutes ago"),
        },
        seconds => match seconds / 3600 {
            1 => "an hour ago".to_owned(),
            hours => format!("{hours} hours ago"),
        },
    }
}

/// `count` with `noun`, pluralised with an "s" when it isn't one: "1 item", "2 items".
pub(crate) fn plural(count: usize, noun: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{} {noun}{suffix}", self::count(count as u64))
}
