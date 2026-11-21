//! Read-only protection for projects with an existing collaboration checkpoint.

use crate::core::error::AppError;
use agentero_core::features::sync::writing_cloud_database;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::Path;

fn has_legacy_checkout(vault_path: &str, project_id: &str) -> Result<bool, AppError> {
    uuid::Uuid::parse_str(project_id).map_err(|_| AppError::message("invalid writing project"))?;
    let vault = crate::core::fs::resolve_vault(vault_path)?.canonicalize()?;
    let database = writing_cloud_database(&vault);
    if !database.exists() {
        return Ok(false);
    }
    if database.symlink_metadata()?.is_symlink() {
        return Err(AppError::message("invalid writing database path"));
    }
    let db = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(db
        .query_row(
            "SELECT 1 FROM writing_checkout WHERE project_id=?1",
            [project_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub(super) fn check_project_git_write(path: &Path) -> Result<(), AppError> {
    // A shared checkout is marked by its portable ID, independent of where its
    // user chooses to place the project in the local classification tree.
    let Some(id) = std::fs::read(path.join("agentero.project.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|config| config["projectId"].as_str().map(str::to_owned))
    else {
        return Ok(());
    };
    for directory in path
        .ancestors()
        .skip(1)
        .filter(|entry| entry.file_name().and_then(|name| name.to_str()) == Some("thesis"))
    {
        if let Some(vault) = directory.parent() {
            if has_legacy_checkout(&vault.to_string_lossy(), &id)? {
                return Err(AppError::domain(
                    "writing_project_shared",
                    "Git sync and Git restore are disabled for a shared writing project.",
                ));
            }
        }
    }
    Ok(())
}
