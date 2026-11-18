//! Atomic staged binary swap and `.old` rollback logic for LeanShell updates and setup.

use std::path::{Path, PathBuf};

/// Executables managed by LeanShell updates and setup in the install folder.
pub const ALL_MANAGED_FILES: [&str; 3] = ["leanshell.exe", "leanshell-settings.exe", "leanshell-setup.exe"];

/// Returns `<target>.old` beside `target`.
pub fn old_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(".old");
    PathBuf::from(name)
}

/// Atomically moves `target` to `<target>.old` and moves `new` into `target`.
pub fn swap(target: &Path, new: &Path) -> Result<(), String> {
    let old = old_path(target);
    let _ = std::fs::remove_file(&old);
    let fail = |error: std::io::Error| format!("Cannot replace {}: {error}", target.display());
    if target.exists() {
        std::fs::rename(target, &old).map_err(fail)?;
    }
    std::fs::rename(new, target).map_err(|error| {
        let _ = std::fs::rename(&old, target);
        fail(error)
    })
}

/// Atomically swaps every `<file>.new` staged in `stage_dir` into `target_dir`,
/// keeping `<file>.old` backups and rolling back all already-swapped files if any step fails.
pub fn apply_staged_dir(stage_dir: &Path, target_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target_dir)
        .map_err(|error| format!("Cannot create target folder {}: {error}", target_dir.display()))?;

    let same_dir = same_canonical(stage_dir, target_dir) || stage_dir == target_dir;
    let mut pairs = Vec::new();
    for file in ALL_MANAGED_FILES {
        let src_new = stage_dir.join(format!("{file}.new"));
        if !src_new.is_file() {
            continue;
        }
        let target = target_dir.join(file);
        let dest_new = if same_dir {
            src_new
        } else {
            let local_new = target_dir.join(format!("{file}.new"));
            copy_or_move(&src_new, &local_new)?;
            local_new
        };
        pairs.push((target, dest_new));
    }
    if pairs.is_empty() {
        return Err(format!("No staged update files found in {}.", stage_dir.display()));
    }

    let mut swapped = Vec::new();
    for (target, new) in &pairs {
        if let Err(error) = swap(target, new) {
            for (done_target, done_old) in swapped
                .iter()
                .rev()
                .filter(|(_, old): &&(PathBuf, PathBuf)| old.exists())
            {
                let _ = std::fs::remove_file(done_target);
                let _ = std::fs::rename(done_old, done_target);
            }
            for (_, rem_new) in &pairs {
                let _ = std::fs::remove_file(rem_new);
            }
            return Err(error);
        }
        swapped.push((target.clone(), old_path(target)));
    }
    Ok(())
}

/// Restores `<file>.old` backups in `target_dir` if the newly swapped build fails to stay alive.
pub fn rollback_dir(target_dir: &Path) -> Result<(), String> {
    for file in ALL_MANAGED_FILES {
        let target = target_dir.join(file);
        let old = old_path(&target);
        if old.is_file() {
            let _ = std::fs::remove_file(&target);
            std::fs::rename(&old, &target)
                .map_err(|error| format!("Rollback failed for {}: {error}", target.display()))?;
        }
    }
    Ok(())
}

fn copy_or_move(src: &Path, dst: &Path) -> Result<(), String> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dst).map_err(|error| format!("Cannot copy staged file {}: {error}", src.display()))?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

fn same_canonical(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a.as_os_str().eq_ignore_ascii_case(b.as_os_str()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_staged_dir_swaps_and_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let stage = dir.path().join("stage");
        let target = dir.path().join("target");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::create_dir_all(&target).unwrap();

        std::fs::write(target.join("leanshell.exe"), b"old launcher").unwrap();
        std::fs::write(target.join("leanshell-settings.exe"), b"old settings").unwrap();
        std::fs::write(target.join("leanshell-setup.exe"), b"old setup").unwrap();

        std::fs::write(stage.join("leanshell.exe.new"), b"new launcher").unwrap();
        std::fs::write(stage.join("leanshell-settings.exe.new"), b"new settings").unwrap();
        std::fs::write(stage.join("leanshell-setup.exe.new"), b"new setup").unwrap();

        apply_staged_dir(&stage, &target).unwrap();
        assert_eq!(std::fs::read(target.join("leanshell.exe")).unwrap(), b"new launcher");
        assert_eq!(
            std::fs::read(target.join("leanshell-settings.exe")).unwrap(),
            b"new settings"
        );
        assert_eq!(std::fs::read(target.join("leanshell-setup.exe")).unwrap(), b"new setup");
        assert_eq!(
            std::fs::read(target.join("leanshell.exe.old")).unwrap(),
            b"old launcher"
        );

        rollback_dir(&target).unwrap();
        assert_eq!(std::fs::read(target.join("leanshell.exe")).unwrap(), b"old launcher");
        assert_eq!(std::fs::read(target.join("leanshell-setup.exe")).unwrap(), b"old setup");
    }

    #[test]
    fn apply_staged_dir_rolls_back_all_if_one_swap_fails() {
        let dir = tempfile::tempdir().unwrap();
        let stage = dir.path().join("stage");
        std::fs::create_dir_all(&stage).unwrap();

        std::fs::write(stage.join("leanshell.exe"), b"v1 launcher").unwrap();
        std::fs::write(stage.join("leanshell.exe.new"), b"v2 launcher").unwrap();
        // A non-empty directory at `.old` cannot be removed or overwritten by `rename`.
        std::fs::create_dir_all(stage.join("leanshell-settings.exe.old/lock")).unwrap();
        std::fs::write(stage.join("leanshell-settings.exe"), b"v1 settings").unwrap();
        std::fs::write(stage.join("leanshell-settings.exe.new"), b"v2 settings").unwrap();

        assert!(apply_staged_dir(&stage, &stage).is_err());
        assert_eq!(std::fs::read(stage.join("leanshell.exe")).unwrap(), b"v1 launcher");
        assert_eq!(
            std::fs::read(stage.join("leanshell-settings.exe")).unwrap(),
            b"v1 settings"
        );
        assert!(!stage.join("leanshell.exe.new").exists());
        assert!(!stage.join("leanshell-settings.exe.new").exists());
    }
}
