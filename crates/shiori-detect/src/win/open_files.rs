//! The files a process has open. Windows has no documented way to ask, so
//! this walks the system-wide handle table with internal calls, as
//! Anisthesia's `win_open_files.cpp` does. Every check below is there
//! because skipping it made Anisthesia hang or misreport on someone's
//! machine.

use std::collections::HashMap;
use std::ffi::c_void;

use windows::Wdk::Foundation::{NtQueryObject, ObjectTypeInformation};
use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
use windows::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, STATUS_INFO_LENGTH_MISMATCH, UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_NAME_NORMALIZED, FILE_TYPE_DISK, GETFINALPATHNAMEBYHANDLE_FLAGS,
    GetFileAttributesW, GetFileType, GetFinalPathNameByHandleW, INVALID_FILE_ATTRIBUTES,
    VOLUME_NAME_DOS,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE};
use windows::core::PCWSTR;

use super::Owned;

/// `SystemHandleInformation` (16) only has 16-bit process ids.
const SYSTEM_EXTENDED_HANDLE_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(64);

const FILE_READ_DATA: u32 = 0x0001;
const FILE_APPEND_DATA: u32 = 0x0004;
const FILE_WRITE_EA: u32 = 0x0010;
const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;

#[repr(C)]
#[derive(Clone, Copy)]
struct HandleEntry {
    object: *mut c_void,
    unique_process_id: usize,
    handle_value: usize,
    granted_access: u32,
    creator_back_trace_index: u16,
    object_type_index: u16,
    handle_attributes: u32,
    reserved: u32,
}

#[repr(C)]
struct HandleTableHeader {
    number_of_handles: usize,
    reserved: usize,
}

#[repr(C)]
struct ObjectTypeInfo {
    type_name: UNICODE_STRING,
    reserved: [u32; 22],
}

#[derive(Default)]
pub struct OpenFiles {
    /// The object type number of files. It changes between Windows
    /// versions, so it is learned from the first file handle seen.
    file_type: Option<u16>,
}

impl OpenFiles {
    /// The open files of each process in `pids`, in handle order. A process
    /// that cannot be opened has none.
    pub fn list(&mut self, pids: &[u32]) -> HashMap<u32, Vec<String>> {
        let mut found: HashMap<u32, Vec<String>> = HashMap::new();
        let processes: HashMap<u32, Owned> = pids
            .iter()
            .filter_map(|&pid| {
                // SAFETY: plain call; the handle is closed by `Owned`.
                let handle = unsafe { OpenProcess(PROCESS_DUP_HANDLE, false, pid) }.ok()?;
                Some((pid, Owned(handle)))
            })
            .collect();
        if processes.is_empty() {
            return found;
        }
        let Some(table) = handle_table() else {
            return found;
        };
        for entry in entries(&table) {
            let pid = entry.unique_process_id as u32;
            let Some(process) = processes.get(&pid) else {
                continue;
            };
            if self.file_type.is_some_and(|t| t != entry.object_type_index) {
                continue;
            }
            if !is_read_only_access(entry.granted_access) {
                continue;
            }
            let Some(handle) = duplicate(process.0, entry.handle_value) else {
                continue;
            };
            if self.file_type.is_none() {
                if type_name(handle.0).as_deref() != Some("File") {
                    continue;
                }
                self.file_type = Some(entry.object_type_index);
            }
            // Pipes, sockets and consoles: their names can hang the query.
            // SAFETY: a valid handle we own.
            if unsafe { GetFileType(handle.0) } != FILE_TYPE_DISK {
                continue;
            }
            let Some(path) = final_path(handle.0) else {
                continue;
            };
            if !is_media_candidate(&path) {
                continue;
            }
            let path = without_long_path_prefix(&path);
            let files = found.entry(pid).or_default();
            if !files.contains(&path) {
                files.push(path);
            }
        }
        found
    }
}

/// Handles that can make `NtQueryObject` and `GetFinalPathNameByHandleW`
/// hang, mostly named pipes, are told apart by their access rights. A
/// player needs to read a video and has no reason to write to it, so a
/// handle without read access, or with write access, is skipped. Blocking
/// single known-bad masks, Anisthesia's first approach, kept missing new
/// ones.
fn is_read_only_access(mask: u32) -> bool {
    mask & FILE_READ_DATA != 0
        && mask & (FILE_APPEND_DATA | FILE_WRITE_EA | FILE_WRITE_ATTRIBUTES) == 0
}

/// Every handle in the system. Starts at 1 MiB and grows up to 16 MiB, as
/// the table grows between asking for its size and reading it.
fn handle_table() -> Option<Vec<u64>> {
    const MAX: usize = 16 << 20;
    let mut size: usize = 1 << 20;
    loop {
        // u64 so that the entries, which hold pointers, are aligned.
        let mut buffer = vec![0u64; size / 8];
        let mut needed = 0u32;
        // SAFETY: the buffer is `size` bytes long, as passed.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buffer.as_mut_ptr().cast(),
                size as u32,
                &mut needed,
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH && size < MAX {
            size = (needed as usize).max(size * 2).min(MAX).next_multiple_of(8);
            continue;
        }
        if status.is_err() {
            tracing::debug!("Could not list handles: {status:?}");
            return None;
        }
        return Some(buffer);
    }
}

/// The entries in the table, never reading past its end even if the count
/// in the header says more.
fn entries(table: &[u64]) -> impl Iterator<Item = HandleEntry> + '_ {
    let bytes = std::mem::size_of_val(table);
    let header = std::mem::size_of::<HandleTableHeader>();
    let entry = std::mem::size_of::<HandleEntry>();
    // SAFETY: the buffer is larger than the header and aligned for it.
    let count = unsafe { (*table.as_ptr().cast::<HandleTableHeader>()).number_of_handles };
    let fits = bytes.saturating_sub(header) / entry;
    let base = table.as_ptr().cast::<u8>();
    (0..count.min(fits)).map(move |i| {
        // SAFETY: entry `i` lies within the buffer, checked above.
        unsafe {
            base.add(header + i * entry)
                .cast::<HandleEntry>()
                .read_unaligned()
        }
    })
}

/// A copy of another process's handle that we can query.
fn duplicate(process: HANDLE, value: usize) -> Option<Owned> {
    let mut copy = HANDLE::default();
    // SAFETY: `process` was opened with PROCESS_DUP_HANDLE. A handle value
    // that closed meanwhile makes the call fail.
    unsafe {
        DuplicateHandle(
            process,
            HANDLE(value as *mut c_void),
            GetCurrentProcess(),
            &mut copy,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
    }
    .ok()?;
    Some(Owned(copy))
}

fn type_name(handle: HANDLE) -> Option<String> {
    let mut size = std::mem::size_of::<ObjectTypeInfo>() as u32 + 512;
    for _ in 0..2 {
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let mut needed = 0u32;
        // SAFETY: the buffer is at least `size` bytes.
        let status = unsafe {
            NtQueryObject(
                Some(handle),
                ObjectTypeInformation,
                Some(buffer.as_mut_ptr().cast()),
                size,
                Some(&mut needed),
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH {
            size = needed;
            continue;
        }
        if status.is_err() {
            return None;
        }
        // SAFETY: on success the buffer starts with the structure, and its
        // name points into the same buffer.
        let name = unsafe {
            let info = &*buffer.as_ptr().cast::<ObjectTypeInfo>();
            let name = &info.type_name;
            if name.Buffer.is_null() {
                return None;
            }
            std::slice::from_raw_parts(name.Buffer.0, usize::from(name.Length) / 2)
        };
        return Some(String::from_utf16_lossy(name));
    }
    None
}

/// The path with a `\\?\` prefix, as Windows gives it.
fn final_path(handle: HANDLE) -> Option<Vec<u16>> {
    let mut buffer = vec![0u16; 260];
    for _ in 0..2 {
        // SAFETY: the buffer is valid for its whole length.
        let len = unsafe {
            GetFinalPathNameByHandleW(
                handle,
                &mut buffer,
                GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
            )
        } as usize;
        if len == 0 {
            return None;
        }
        if len < buffer.len() {
            buffer.truncate(len);
            return Some(buffer);
        }
        // Too small: `len` is the size needed, with the terminating zero.
        buffer = vec![0u16; len];
    }
    None
}

/// Skips the Windows folder, folders and files that no longer exist.
fn is_media_candidate(path: &[u16]) -> bool {
    let text = String::from_utf16_lossy(path);
    if super::windows_directory().is_some_and(|dir| super::is_under(&text, &dir)) {
        return false;
    }
    let mut terminated = path.to_vec();
    terminated.push(0);
    // SAFETY: the string is terminated by the zero just added.
    let attributes = unsafe { GetFileAttributesW(PCWSTR(terminated.as_ptr())) };
    attributes != INVALID_FILE_ATTRIBUTES && attributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
}

/// `\\?\C:\a.mkv` to `C:\a.mkv` and `\\?\UNC\nas\a.mkv` to `\\nas\a.mkv`:
/// the form people and the window title strategy use.
fn without_long_path_prefix(path: &[u16]) -> String {
    let text = String::from_utf16_lossy(path);
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_masks() {
        // The usual mask of a video a player has open.
        assert!(is_read_only_access(0x0012_0089));
        // SYNCHRONIZE only, and a named pipe's mask.
        assert!(!is_read_only_access(0x0010_0000));
        assert!(!is_read_only_access(0x0012_019f));
    }

    #[test]
    fn long_path_prefix() {
        let wide = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        assert_eq!(
            without_long_path_prefix(&wide(r"\\?\C:\a.mkv")),
            r"C:\a.mkv"
        );
        assert_eq!(
            without_long_path_prefix(&wide(r"\\?\UNC\nas\share\a.mkv")),
            r"\\nas\share\a.mkv"
        );
    }

    #[test]
    fn finds_a_file_this_process_has_open() {
        let path = std::env::current_exe()
            .unwrap()
            .with_file_name(format!("open-files-test-{}.mkv", std::process::id()));
        std::fs::write(&path, b"x").unwrap();
        let _file = std::fs::File::open(&path).unwrap();
        let me = std::process::id();
        let files = OpenFiles::default().list(&[me]);
        let expected = std::fs::canonicalize(&path).unwrap();
        let expected = without_long_path_prefix(
            &expected
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .collect::<Vec<_>>(),
        );
        assert!(
            files.get(&me).is_some_and(|f| f.contains(&expected)),
            "{expected} not in {files:?}"
        );
    }
}
