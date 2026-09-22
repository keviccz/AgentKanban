use crate::{Error, Result};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIdentity {
    pub identity: String,
    pub name: String,
    pub path: String,
}

/// Git worktrees share the canonical common git directory; other directories stay distinct.
pub fn resolve_project(value: &str) -> Result<ProjectIdentity> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(Error::InvalidInput(
            "project_path must be an absolute existing directory".into(),
        ));
    }
    let requested = Path::new(value);
    if !requested.is_absolute() {
        return Err(Error::InvalidInput("project_path must be absolute".into()));
    }
    let directory = requested
        .canonicalize()
        .map_err(|error| Error::ProjectIdentity(format!("{value}: {error}")))?;
    if !directory.is_dir() {
        return Err(Error::InvalidInput(
            "project_path must identify a directory, not a file".into(),
        ));
    }
    let probe = git(
        &directory,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    let (identity_path, display_path, prefix) = match probe {
        Ok(output) if output.status.success() => {
            let output_path = String::from_utf8(output.stdout)
                .map_err(|_| Error::ProjectIdentity("git returned a non-UTF-8 path".into()))?;
            let common_dir = PathBuf::from(output_path.trim()).canonicalize()?;
            let display = if common_dir.file_name().is_some_and(|name| name == ".git") {
                common_dir.parent().unwrap_or(&common_dir).to_path_buf()
            } else {
                // Also support --separate-git-dir and bare repositories.
                match git(&directory, &["rev-parse", "--show-toplevel"]) {
                    Ok(output) if output.status.success() => {
                        let raw = String::from_utf8(output.stdout).map_err(|_| {
                            Error::ProjectIdentity("git returned a non-UTF-8 path".into())
                        })?;
                        PathBuf::from(raw.trim()).canonicalize()?
                    }
                    _ => common_dir.clone(),
                }
            };
            (common_dir, display, "git")
        }
        Ok(output) if String::from_utf8_lossy(&output.stderr).contains("not a git repository") => {
            (directory.clone(), directory, "dir")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (directory.clone(), directory, "dir")
        }
        Ok(output) => {
            return Err(Error::ProjectIdentity(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
        Err(error) => return Err(Error::ProjectIdentity(error.to_string())),
    };
    let path = display_path_string(&display_path)?;
    let identity = display_path_string(&identity_path)?;
    #[cfg(windows)]
    let identity = identity.to_lowercase();
    let name = display_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.clone());
    Ok(ProjectIdentity {
        identity: format!("{prefix}:{identity}"),
        name,
        path,
    })
}

fn display_path_string(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::ProjectIdentity("project path is not valid Unicode".into()))?;
    #[cfg(windows)]
    {
        if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
            return Ok(format!("\\\\{unc}"));
        }
        if let Some(plain) = text.strip_prefix("\\\\?\\") {
            return Ok(plain.to_string());
        }
    }
    Ok(text.to_string())
}

fn git(directory: &Path, args: &[&str]) -> std::io::Result<Output> {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(args);
    // Do not inherit the caller's repository overrides: project_path is authoritative.
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(variable);
    }
    command.env("LC_ALL", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW, including when launched by the GUI.
    }
    command.output()
}
