//! The handful of file operations Goby needs, straight on Win32. Paths are plain strings:
//! std::fs and std::path would add several KB to the executable for these few calls.

use std::io;

use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CreateDirectoryW, CreateFileW, DeleteFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_CREATION_DISPOSITION, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FindClose, FindFirstFileW, FindNextFileW, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
    MOVEFILE_REPLACE_EXISTING, MoveFileExW, OPEN_EXISTING, ReadFile, RemoveDirectoryW, WIN32_FIND_DATAW, WriteFile,
};
use windows::core::HSTRING;

/// The error a failed call left, as std::fs would report it.
fn os_error(e: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(e.code().0 & 0xffff)
}

fn attributes(path: &str) -> u32 {
    unsafe { GetFileAttributesW(&HSTRING::from(path)) }
}

fn is_dir(path: &str) -> bool {
    let a = attributes(path);
    a != INVALID_FILE_ATTRIBUTES && a & FILE_ATTRIBUTE_DIRECTORY.0 != 0
}

/// Creates the folder and any parents it lacks.
pub fn create_dirs(path: &str) -> io::Result<()> {
    // Each parent in turn; the ones that are there already just fail.
    for (i, _) in path.match_indices('\\') {
        unsafe {
            let _ = CreateDirectoryW(&HSTRING::from(&path[..i]), None);
        }
    }
    match unsafe { CreateDirectoryW(&HSTRING::from(path), None) } {
        Err(e) if !is_dir(path) => Err(os_error(e)),
        _ => Ok(()),
    }
}

pub fn remove_file(path: &str) -> bool {
    unsafe { DeleteFileW(&HSTRING::from(path)).is_ok() }
}

/// Moves `from` to `to`, replacing whatever is there.
pub fn rename(from: &str, to: &str) -> io::Result<()> {
    unsafe { MoveFileExW(&HSTRING::from(from), &HSTRING::from(to), MOVEFILE_REPLACE_EXISTING) }.map_err(os_error)
}

/// Deletes a folder and everything in it. A link inside is removed, never followed.
pub fn remove_dir_all(path: &str) {
    unsafe {
        let mut found = WIN32_FIND_DATAW::default();
        if let Ok(find) = FindFirstFileW(&HSTRING::from(format!("{path}\\*")), &mut found) {
            loop {
                let len = found.cFileName.iter().position(|c| *c == 0).unwrap_or(found.cFileName.len());
                let name = String::from_utf16_lossy(&found.cFileName[..len]);
                if name != "." && name != ".." {
                    let child = format!("{path}\\{name}");
                    let a = found.dwFileAttributes;
                    if a & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
                        remove_file(&child);
                    } else if a & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                        let _ = RemoveDirectoryW(&HSTRING::from(child));
                    } else {
                        remove_dir_all(&child);
                    }
                }
                if FindNextFileW(find, &mut found).is_err() {
                    break;
                }
            }
            let _ = FindClose(find);
        }
        let _ = RemoveDirectoryW(&HSTRING::from(path));
    }
}

/// An open file, closed when dropped.
pub struct File(HANDLE);

// A file handle can be used and closed from any thread.
unsafe impl Send for File {}

impl Drop for File {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

impl File {
    fn open(path: &str, access: u32, share: FILE_SHARE_MODE, how: FILE_CREATION_DISPOSITION) -> Option<File> {
        unsafe { CreateFileW(&HSTRING::from(path), access, share, None, how, FILE_ATTRIBUTE_NORMAL, None) }
            .ok()
            .map(File)
    }

    /// Opens `path` for reading while letting others read it too, but not write or delete it.
    pub fn locked(path: &str) -> Option<File> {
        File::open(path, GENERIC_READ.0, FILE_SHARE_READ, OPEN_EXISTING)
    }

    /// Everything from the current position to the end.
    pub fn read_all(&self) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut n = 0u32;
            unsafe { ReadFile(self.0, Some(&mut buf), Some(&mut n), None) }.ok()?;
            if n == 0 {
                return Some(out);
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
    }
}

/// Shared the way std::fs opens files, so an editor holding the file open does not matter.
const SHARE_ALL: FILE_SHARE_MODE = FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);

pub fn read(path: &str) -> Option<Vec<u8>> {
    File::open(path, GENERIC_READ.0, SHARE_ALL, OPEN_EXISTING)?.read_all()
}

/// Creates or replaces `path` with `data`.
pub fn write(path: &str, data: &[u8]) -> bool {
    let Some(file) = File::open(path, GENERIC_WRITE.0, SHARE_ALL, CREATE_ALWAYS) else { return false };
    let mut rest = data;
    while !rest.is_empty() {
        let mut n = 0u32;
        if unsafe { WriteFile(file.0, Some(rest), Some(&mut n), None) }.is_err() || n == 0 {
            return false;
        }
        rest = &rest[n as usize..];
    }
    true
}
