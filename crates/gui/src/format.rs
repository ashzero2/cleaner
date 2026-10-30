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

pub(crate) fn name(path: &Path) -> String {
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
