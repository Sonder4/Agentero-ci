//! Cross-process exclusive lock stored beside a configuration file.
//!
//! The lock file is deliberately separate from the target because atomic
//! replacement renames a new inode/file over the target. Locking the target
//! itself would therefore serialize operations on different files.

use crate::error::AppError;
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

/// An exclusive advisory lock held until this value is dropped.
pub struct SidecarLock {
    file: File,
}

impl SidecarLock {
    /// Acquire `<target>.lock`, creating the parent directory when needed.
    ///
    /// This blocks until another Agentero process releases the lock.
    pub fn acquire(target: &Path) -> Result<Self, AppError> {
        let path = lock_path(target)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }
}

impl Drop for SidecarLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn lock_path(target: &Path) -> Result<PathBuf, AppError> {
    let name = target
        .file_name()
        .ok_or_else(|| AppError::message("lock target has no file name"))?;
    let lock_name = format!("{}.lock", name.to_string_lossy());
    Ok(target.with_file_name(lock_name))
}
