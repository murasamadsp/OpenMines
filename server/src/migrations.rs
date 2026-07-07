use anyhow::Result;
use std::env;
use std::path::{Path, PathBuf};

const DB_FILENAME: &str = "openmines.db";

pub fn resolve_state_dir(config_data_dir: &str, env_data_dir: Option<String>) -> Result<PathBuf> {
    resolve_state_dir_from(env_data_dir, &env::current_dir()?, config_data_dir)
}

fn resolve_state_dir_from(
    env_override: Option<String>,
    current_dir: &Path,
    config_data_dir: &str,
) -> Result<PathBuf> {
    if let Some(raw) = env_override {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("M3R_DATA_DIR is set but empty");
        }
        return Ok(PathBuf::from(trimmed));
    }
    let trimmed = config_data_dir.trim();
    if trimmed.is_empty() {
        anyhow::bail!("config data_dir is empty");
    }
    Ok(current_dir.join(trimmed))
}

/// Переносит БД и слои мира из рабочего каталога (старая схема) в `state_dir`.
pub fn migrate_legacy_state_files(state_dir: &Path, world_name: &str) -> Result<()> {
    let cwd = env::current_dir()?;
    let names = [
        "mines3.db".to_string(),
        "mines3.db-wal".to_string(),
        "mines3.db-shm".to_string(),
        "openmines.db".to_string(),
        "openmines.db-wal".to_string(),
        "openmines.db-shm".to_string(),
        format!("{world_name}_v2.map"),
        format!("{world_name}_durability.map"),
    ];
    for name in names {
        let from = cwd.join(&name);
        let to = state_dir.join(&name);
        if to.exists() || !from.exists() {
            continue;
        }
        match std::fs::rename(&from, &to) {
            Ok(()) => tracing::info!(
                file_name = %name,
                state_dir = %state_dir.display(),
                "Migrated legacy state file"
            ),
            Err(err) => tracing::warn!(
                file_name = %name,
                state_dir = %state_dir.display(),
                error = ?err,
                "Could not migrate legacy state file"
            ),
        }
    }
    Ok(())
}

/// Переименование legacy `mines3.db*` → `openmines.db*` внутри каталога состояния.
pub fn migrate_mines3_db_to_openmines(state_dir: &Path) {
    let new_main = state_dir.join(DB_FILENAME);
    if new_main.exists() {
        return;
    }
    let old_main = state_dir.join("mines3.db");
    if !old_main.exists() {
        return;
    }
    for (old_suffix, new_suffix) in [
        ("mines3.db", DB_FILENAME),
        ("mines3.db-wal", "openmines.db-wal"),
        ("mines3.db-shm", "openmines.db-shm"),
    ] {
        let from = state_dir.join(old_suffix);
        let to = state_dir.join(new_suffix);
        if from.exists() && !to.exists() {
            match std::fs::rename(&from, &to) {
                Ok(()) => tracing::info!(
                    from = %from.display(),
                    to = %to.display(),
                    "Renamed legacy database file"
                ),
                Err(e) => tracing::warn!(
                    from = %from.display(),
                    error = ?e,
                    "Could not rename legacy database file"
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_state_dir_uses_explicit_env_override() {
        let cwd = PathBuf::from("/repo");
        assert_eq!(
            resolve_state_dir_from(Some(" /tmp/openmines ".to_string()), &cwd, "data").unwrap(),
            PathBuf::from("/tmp/openmines")
        );
    }

    #[test]
    fn test_resolve_state_dir_uses_config_data_dir_without_env() {
        let cwd = PathBuf::from("/repo");
        assert_eq!(
            resolve_state_dir_from(None, &cwd, " data ").unwrap(),
            PathBuf::from("/repo/data")
        );
    }

    #[test]
    fn test_resolve_state_dir_empty_values_are_errors_not_current_dir() {
        let cwd = PathBuf::from("/repo");
        assert!(resolve_state_dir_from(Some(String::new()), &cwd, "data").is_err());
        assert!(resolve_state_dir_from(Some("   ".to_string()), &cwd, "data").is_err());
        assert!(resolve_state_dir_from(None, &cwd, " ").is_err());
    }
}
