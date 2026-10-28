use std::io;
use std::path::{Component, Path};
use std::process::{Command, Stdio};

use trash::TrashContext;
use trash::macos::{DeleteMethod, TrashContextExtMacos};

use crate::error::Error;
use crate::rules::Finding;

const MIN_DEPTH: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Trashed,
    CommandRan,
    AlreadyGone,
}

/// Cleans one finding: runs the rule's command when it has one and the program is
/// installed, otherwise moves the path to the Trash.
///
/// # Errors
///
/// Fails if the path looks unsafe to touch (too close to the filesystem root, or a
/// symlink), the command fails, or the Trash move fails.
pub fn clean(finding: &Finding<'_>) -> Result<Outcome, Error> {
    let path = &finding.path;
    refuse_unsafe(path)?;

    if let Some(argv) = &finding.rule.command
        && let Some(outcome) = run_command(argv)?
    {
        return Ok(outcome);
    }

    match path.symlink_metadata() {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Outcome::AlreadyGone),
        Err(_) => return Err(refused(path, "cannot inspect path")),
    }
    let mut context = TrashContext::default();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context.delete(path).map_err(|source| Error::Trash {
        path: path.clone(),
        source,
    })?;
    Ok(Outcome::Trashed)
}

fn refuse_unsafe(path: &Path) -> Result<(), Error> {
    let depth = path
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count();
    if !path.is_absolute() || depth < MIN_DEPTH {
        return Err(refused(path, "too close to the filesystem root"));
    }
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_symlink() => Err(refused(path, "is a symlink")),
        _ => Ok(()),
    }
}

fn refused(path: &Path, reason: &'static str) -> Error {
    Error::Refused {
        path: path.to_path_buf(),
        reason,
    }
}

fn run_command(argv: &[String]) -> Result<Option<Outcome>, Error> {
    let Some((executable, rest)) = argv.split_first() else {
        return Ok(None);
    };
    let status = Command::new(executable)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let command = argv.join(" ");
    match status {
        Ok(status) if status.success() => Ok(Some(Outcome::CommandRan)),
        Ok(status) => Err(Error::CommandFailed { command, status }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Command { command, source }),
    }
}
