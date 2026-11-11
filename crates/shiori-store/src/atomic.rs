//! Writing files so that a crash never leaves a half-written file behind.

use std::io::Write;
use std::path::Path;

/// Writes to a temporary file next to `path`, then renames it over `path`.
pub fn write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    write_with(path, contents, |_| Ok(()))
}

/// Like [`write`], with a hook to adjust the temporary file (for example its
/// permissions) before it takes the place of the old one.
pub fn write_with(
    path: &Path,
    contents: &[u8],
    prepare: impl FnOnce(&std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile_in(dir, path)?;
    prepare(&tmp.1)?;
    tmp.1.write_all(contents)?;
    tmp.1.sync_all()?;
    drop(tmp.1);
    std::fs::rename(&tmp.0, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp.0);
    })
}

fn tempfile_in(dir: &Path, path: &Path) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)?;
    Ok((tmp, file))
}
