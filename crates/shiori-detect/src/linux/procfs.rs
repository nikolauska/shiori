//! Processes and their open files, from `/proc`.

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    /// The program's file name, such as `mpv`.
    pub name: String,
}

/// The current user's processes. Other users' processes are skipped:
/// their open files cannot be read anyway, and a player someone else runs
/// is not one this user is watching.
pub fn processes() -> Vec<Process> {
    let Ok(uid) = fs::metadata("/proc/self").map(|m| m.uid()) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            // The process may have ended since the listing; skip it then.
            if entry.metadata().ok()?.uid() != uid {
                return None;
            }
            let name = program_name(&entry.path())?;
            Some(Process { pid, name })
        })
        .collect()
}

/// From the program's path, so that a player renamed in `argv[0]` or with
/// a name longer than `comm`'s 15 characters still matches. `comm` only
/// when the link cannot be read.
fn program_name(dir: &Path) -> Option<String> {
    if let Ok(exe) = fs::read_link(dir.join("exe")) {
        let exe = exe.to_string_lossy();
        // The program file was replaced while running, as by an update.
        let exe = exe.strip_suffix(" (deleted)").unwrap_or(&exe);
        let name = exe.rsplit('/').next().unwrap_or(exe);
        if !name.is_empty() {
            return Some(name.to_owned());
        }
    }
    let comm = fs::read_to_string(dir.join("comm")).ok()?;
    let comm = comm.trim_end_matches('\n');
    (!comm.is_empty()).then(|| comm.to_owned())
}

/// Folders whose files are never media: the system, and the player's own
/// caches and settings. A player keeps fonts, libraries, databases and
/// logs open the whole time it runs.
fn ignored_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = [
        "/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/opt", "/proc", "/dev", "/sys", "/run",
        "/tmp", "/var", "/nix", "/snap", "/memfd:",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg = |var: &str, fallback: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.as_ref().map(|h| h.join(fallback)))
    };
    folders.extend(xdg("XDG_CACHE_HOME", ".cache"));
    folders.extend(xdg("XDG_CONFIG_HOME", ".config"));
    folders.extend(xdg("XDG_STATE_HOME", ".local/state"));
    // Flatpak apps keep their caches and settings here instead.
    folders.extend(home.map(|h| h.join(".var/app")));
    folders
}

/// The regular files `pid` has open, outside the ignored folders, in
/// descriptor order, each once.
pub fn open_files(pid: u32) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fd")) else {
        return Vec::new();
    };
    let ignored = ignored_folders();
    let mut fds: Vec<(u32, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let fd = entry.file_name().to_str()?.parse().ok()?;
            Some((fd, fs::read_link(entry.path()).ok()?))
        })
        .collect();
    fds.sort_by_key(|(fd, _)| *fd);
    let mut files: Vec<PathBuf> = Vec::new();
    for (_, target) in fds {
        // Pipes and sockets are "pipe:[123]" and such, not paths.
        if !target.is_absolute() || ignored.iter().any(|dir| target.starts_with(dir)) {
            continue;
        }
        // A deleted file's link ends in " (deleted)" and no longer exists,
        // so the check below drops it too.
        if !fs::metadata(&target).is_ok_and(|m| m.is_file()) {
            continue;
        }
        if !files.contains(&target) {
            files.push(target);
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_this_process_and_a_file_it_has_open() {
        let me = std::process::id();
        let found = processes().into_iter().find(|p| p.pid == me).unwrap();
        let exe = std::env::current_exe().unwrap();
        assert_eq!(found.name, exe.file_name().unwrap().to_str().unwrap());

        // Somewhere not ignored: next to the test program, not /tmp.
        let path = exe.with_file_name(format!("open-files-test-{me}.mkv"));
        fs::write(&path, b"x").unwrap();
        let _file = fs::File::open(&path).unwrap();
        let files = open_files(me);
        assert!(files.contains(&path), "{files:?}");
        assert!(files.iter().all(|f| !f.starts_with("/proc")));
    }
}
