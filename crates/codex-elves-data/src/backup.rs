use anyhow::Context;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoBackupTarget {
    pub token: String,
    pub source_db: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoBackupPlan {
    pub session_id: String,
    pub targets: Vec<UndoBackupTarget>,
}

#[derive(Debug, Clone)]
pub struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn write_backup(
        &self,
        session_id: &str,
        source_db: &Path,
        tables: serde_json::Value,
    ) -> anyhow::Result<String> {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let token = format!("{epoch}-{}", Uuid::new_v4().simple());
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create backup directory {}",
                self.root.to_string_lossy()
            )
        })?;
        let payload = json!({
            "token": token,
            "session_id": session_id,
            "source_db": source_db.to_string_lossy(),
            "tables": tables,
        });
        fs::write(
            self.path_for(&token),
            serde_json::to_string_pretty(&payload)?,
        )?;
        Ok(token)
    }

    pub fn read_backup(&self, token: &str) -> anyhow::Result<serde_json::Value> {
        let path = self.path_for(token);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("Backup token not found: {token}"))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn write_multi_database_undo(
        &self,
        session_id: &str,
        backup_tokens: &[String],
    ) -> anyhow::Result<String> {
        if backup_tokens.len() < 2 {
            anyhow::bail!("multi-database undo requires at least two backups");
        }
        let targets = backup_tokens
            .iter()
            .map(|token| self.single_undo_target(token))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let token = self.new_token();
        let payload = json!({
            "token": token,
            "session_id": session_id,
            "kind": "multi_database_delete",
            "targets": targets.iter().map(|target| json!({
                "token": target.token,
                "source_db": target.source_db.to_string_lossy(),
            })).collect::<Vec<_>>(),
        });
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create backup directory {}",
                self.root.to_string_lossy()
            )
        })?;
        fs::write(
            self.path_for(&token),
            serde_json::to_string_pretty(&payload)?,
        )?;
        Ok(token)
    }

    pub fn undo_plan(&self, token: &str) -> anyhow::Result<UndoBackupPlan> {
        let backup = self.read_backup(token)?;
        let session_id = backup["session_id"].as_str().unwrap_or("").to_string();
        if backup.get("kind").and_then(serde_json::Value::as_str) != Some("multi_database_delete") {
            return Ok(UndoBackupPlan {
                session_id,
                targets: vec![self.single_undo_target(token)?],
            });
        }

        let targets = backup["targets"]
            .as_array()
            .context("multi-database undo backup has no targets")?
            .iter()
            .map(|target| {
                let token = target["token"]
                    .as_str()
                    .filter(|token| !token.trim().is_empty())
                    .context("multi-database undo target has no token")?;
                let source_db = target["source_db"]
                    .as_str()
                    .filter(|path| !path.trim().is_empty())
                    .context("multi-database undo target has no source database")?;
                let resolved = self.single_undo_target(token)?;
                if resolved.source_db != PathBuf::from(source_db) {
                    anyhow::bail!(
                        "multi-database undo target source database does not match backup"
                    );
                }
                Ok(resolved)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        if targets.len() < 2 {
            anyhow::bail!("multi-database undo backup has fewer than two targets");
        }
        Ok(UndoBackupPlan {
            session_id,
            targets,
        })
    }

    pub fn path_for(&self, token: &str) -> PathBuf {
        let safe: String = token
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
            .collect();
        self.root.join(format!("{safe}.json"))
    }

    fn new_token(&self) -> String {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{epoch}-{}", Uuid::new_v4().simple())
    }

    fn single_undo_target(&self, token: &str) -> anyhow::Result<UndoBackupTarget> {
        let backup = self.read_backup(token)?;
        let source_db = backup["source_db"]
            .as_str()
            .filter(|path| !path.trim().is_empty())
            .context("backup has no source database")?;
        Ok(UndoBackupTarget {
            token: token.to_string(),
            source_db: PathBuf::from(source_db),
        })
    }
}
