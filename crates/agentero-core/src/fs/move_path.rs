//! Same-volume moves that also work for directory trees on Windows.

use std::io;
use std::path::Path;

/// Move `from` to `to` on the same volume. Callers check that `to` is absent.
///
/// On Windows `std::fs::rename` renames through POSIX semantics
/// (`FileRenameInfoEx`). Some systems (seen on Windows 11 24H2) refuse that
/// with access denied for a directory that contains subdirectories, while
/// `MoveFileExW`, which Explorer and PowerShell use, succeeds. Access denied is
/// therefore retried with `MoveFileExW`; it never replaces an existing target.
/// If the retry also fails, the original error is returned.
pub fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            move_file_ex(from, to).map_err(|_| error)
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn move_file_ex(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>()
    };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both buffers are NUL-terminated UTF-16 paths that outlive the call.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
