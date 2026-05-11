use crate::config::APP_NAME;
use crate::models::Prompt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

pub const PROMPT_BACKUP_SCHEMA_VERSION: u32 = 1;
pub const PROMPT_BACKUP_FILE_EXTENSION: &str = "json";
pub const PROMPT_BACKUP_DEFAULT_FILE_NAME: &str = "jame-prompt-backup.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptBackup {
    pub app_name: String,
    pub app_version: String,
    pub schema_version: u32,
    pub prompts: Vec<Prompt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    Merge,
    ReplaceAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateMode {
    Skip,
    Overwrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptImportPreview {
    pub imported_count: usize,
    pub duplicate_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportOperations {
    pub to_insert: Vec<Prompt>,
    pub to_overwrite: Vec<PromptOverwrite>,
    pub skipped_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PromptOverwrite {
    pub existing_id: String,
    pub prompt: Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub inserted: usize,
    pub overwritten: usize,
    pub skipped: usize,
}

#[derive(Debug)]
pub enum PromptBackupError {
    Json(serde_json::Error),
    UnsupportedSchemaVersion(u32),
    EmptyBackup,
    InvalidPrompt(String),
}

impl fmt::Display for PromptBackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "backup JSON error: {}", error),
            Self::UnsupportedSchemaVersion(version) => {
                write!(f, "unsupported backup schema version: {}", version)
            }
            Self::EmptyBackup => write!(f, "backup does not contain prompts"),
            Self::InvalidPrompt(message) => write!(f, "invalid prompt in backup: {}", message),
        }
    }
}

impl std::error::Error for PromptBackupError {}

impl From<serde_json::Error> for PromptBackupError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl PromptBackup {
    pub fn new(app_version: &str, prompts: Vec<Prompt>) -> Self {
        Self {
            app_name: APP_NAME.to_string(),
            app_version: app_version.to_string(),
            schema_version: PROMPT_BACKUP_SCHEMA_VERSION,
            prompts,
        }
    }

    pub fn to_json(&self) -> Result<String, PromptBackupError> {
        self.validate_for_export()?;
        serde_json::to_string_pretty(self).map_err(PromptBackupError::Json)
    }

    pub fn from_json(json: &str) -> Result<Self, PromptBackupError> {
        let backup: Self = serde_json::from_str(json)?;
        backup.validate()?;
        Ok(backup)
    }

    pub fn validate(&self) -> Result<(), PromptBackupError> {
        self.validate_schema()?;
        if self.prompts.is_empty() {
            return Err(PromptBackupError::EmptyBackup);
        }
        self.validate_prompts()
    }

    fn validate_for_export(&self) -> Result<(), PromptBackupError> {
        self.validate_schema()?;
        self.validate_prompts()
    }

    fn validate_schema(&self) -> Result<(), PromptBackupError> {
        if self.schema_version != PROMPT_BACKUP_SCHEMA_VERSION {
            return Err(PromptBackupError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        Ok(())
    }

    fn validate_prompts(&self) -> Result<(), PromptBackupError> {
        let mut names = HashSet::new();
        for prompt in &self.prompts {
            let name = prompt.name.trim();
            if name.is_empty() {
                return Err(PromptBackupError::InvalidPrompt(
                    "prompt name is required".to_string(),
                ));
            }
            if prompt.content.trim().is_empty() {
                return Err(PromptBackupError::InvalidPrompt(format!(
                    "prompt content is required for {}",
                    prompt.name
                )));
            }
            let normalized = name.to_lowercase();
            if !names.insert(normalized) {
                return Err(PromptBackupError::InvalidPrompt(format!(
                    "duplicate prompt name in backup: {}",
                    prompt.name
                )));
            }
        }
        Ok(())
    }
}

impl PromptImportPreview {
    pub fn from_backup(
        backup: &PromptBackup,
        existing_prompts: &[Prompt],
    ) -> Result<Self, PromptBackupError> {
        backup.validate()?;
        let existing_names: HashSet<String> = existing_prompts
            .iter()
            .map(|prompt| prompt.name.trim().to_lowercase())
            .collect();
        let mut duplicate_names: Vec<String> = backup
            .prompts
            .iter()
            .filter(|prompt| existing_names.contains(&prompt.name.trim().to_lowercase()))
            .map(|prompt| prompt.name.clone())
            .collect();
        duplicate_names.sort_by_key(|name| name.to_lowercase());

        Ok(Self {
            imported_count: backup.prompts.len(),
            duplicate_names,
        })
    }
}

pub fn plan_import_operations(
    backup: &PromptBackup,
    existing_prompts: &[Prompt],
    import_mode: ImportMode,
    duplicate_mode: DuplicateMode,
) -> Result<ImportOperations, PromptBackupError> {
    backup.validate()?;
    if matches!(import_mode, ImportMode::ReplaceAll) {
        return Ok(ImportOperations {
            to_insert: backup.prompts.clone(),
            to_overwrite: Vec::new(),
            skipped_names: Vec::new(),
        });
    }

    let existing_by_name: HashMap<String, &Prompt> = existing_prompts
        .iter()
        .map(|prompt| (prompt.name.trim().to_lowercase(), prompt))
        .collect();

    let mut operations = ImportOperations {
        to_insert: Vec::new(),
        to_overwrite: Vec::new(),
        skipped_names: Vec::new(),
    };

    for prompt in &backup.prompts {
        let normalized_name = prompt.name.trim().to_lowercase();
        if let Some(existing) = existing_by_name.get(&normalized_name) {
            match duplicate_mode {
                DuplicateMode::Skip => operations.skipped_names.push(prompt.name.clone()),
                DuplicateMode::Overwrite => operations.to_overwrite.push(PromptOverwrite {
                    existing_id: existing.id.clone(),
                    prompt: prompt.clone(),
                }),
            }
        } else {
            operations.to_insert.push(prompt.clone());
        }
    }

    operations
        .skipped_names
        .sort_by_key(|name| name.to_lowercase());
    Ok(operations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Prompt;

    fn prompt(name: &str) -> Prompt {
        Prompt {
            id: format!("{name}-id"),
            name: name.to_string(),
            content: format!("{name} content"),
            hotkey: Some(format!("Ctrl+{}", &name[0..1])),
            hotkey_enabled: true,
            favorite: true,
            created_at: "2026-05-11 10:00:00".to_string(),
            updated_at: "2026-05-11 10:30:00".to_string(),
            last_used_at: Some("2026-05-11 10:45:00".to_string()),
            use_count: 7,
        }
    }

    #[test]
    fn prompt_backup_serializes_all_prompt_metadata() {
        let backup = PromptBackup::new("1.1.0", vec![prompt("Alpha")]);

        let json = backup.to_json().expect("Backup should serialize");
        let restored = PromptBackup::from_json(&json).expect("Backup should deserialize");
        let restored_prompt = restored.prompts.first().expect("Prompt should exist");

        assert_eq!(restored.app_name, "JamePrompt");
        assert_eq!(restored.schema_version, PROMPT_BACKUP_SCHEMA_VERSION);
        assert_eq!(restored_prompt.id, "Alpha-id");
        assert_eq!(restored_prompt.hotkey, Some("Ctrl+A".to_string()));
        assert!(restored_prompt.favorite);
        assert_eq!(restored_prompt.use_count, 7);
        assert_eq!(
            restored_prompt.last_used_at,
            Some("2026-05-11 10:45:00".to_string())
        );
    }

    #[test]
    fn prompt_backup_rejects_unknown_schema_version() {
        let json =
            r#"{"app_name":"JamePrompt","app_version":"9.9.9","schema_version":999,"prompts":[]}"#;

        let error = PromptBackup::from_json(json).expect_err("Unknown schema should fail");

        assert!(matches!(
            error,
            PromptBackupError::UnsupportedSchemaVersion(999)
        ));
    }

    #[test]
    fn prompt_backup_rejects_empty_prompt_names_or_content() {
        let mut backup = PromptBackup::new("1.1.0", vec![prompt("Alpha")]);
        backup.prompts[0].name.clear();

        let error = backup.validate().expect_err("Empty names should fail");

        assert!(
            matches!(error, PromptBackupError::InvalidPrompt(message) if message.contains("name"))
        );
    }

    #[test]
    fn import_preview_detects_duplicate_prompt_names() {
        let existing = vec![prompt("Alpha"), prompt("Beta")];
        let incoming = PromptBackup::new("1.1.0", vec![prompt("Alpha"), prompt("Gamma")]);

        let preview = PromptImportPreview::from_backup(&incoming, &existing)
            .expect("Preview should be created");

        assert_eq!(preview.imported_count, 2);
        assert_eq!(preview.duplicate_names, vec!["Alpha".to_string()]);
    }

    #[test]
    fn import_merge_skips_duplicate_prompts_when_requested() {
        let existing = vec![prompt("Alpha")];
        let incoming = PromptBackup::new("1.1.0", vec![prompt("Alpha"), prompt("Beta")]);

        let operations =
            plan_import_operations(&incoming, &existing, ImportMode::Merge, DuplicateMode::Skip)
                .expect("Operations should be planned");

        assert_eq!(operations.to_insert.len(), 1);
        assert_eq!(operations.to_insert[0].name, "Beta");
        assert!(operations.to_overwrite.is_empty());
        assert_eq!(operations.skipped_names, vec!["Alpha".to_string()]);
    }

    #[test]
    fn import_merge_overwrites_duplicate_prompts_when_requested() {
        let existing = vec![prompt("Alpha")];
        let incoming = PromptBackup::new("1.1.0", vec![prompt("Alpha"), prompt("Beta")]);

        let operations = plan_import_operations(
            &incoming,
            &existing,
            ImportMode::Merge,
            DuplicateMode::Overwrite,
        )
        .expect("Operations should be planned");

        assert_eq!(operations.to_insert.len(), 1);
        assert_eq!(operations.to_overwrite.len(), 1);
        assert_eq!(operations.to_overwrite[0].existing_id, "Alpha-id");
    }

    #[test]
    fn import_replace_all_imports_every_prompt_without_duplicate_overwrite() {
        let existing = vec![prompt("Alpha")];
        let incoming = PromptBackup::new("1.1.0", vec![prompt("Alpha"), prompt("Beta")]);

        let operations = plan_import_operations(
            &incoming,
            &existing,
            ImportMode::ReplaceAll,
            DuplicateMode::Skip,
        )
        .expect("Operations should be planned");

        assert_eq!(operations.to_insert.len(), 2);
        assert!(operations.to_overwrite.is_empty());
        assert!(operations.skipped_names.is_empty());
    }
}
