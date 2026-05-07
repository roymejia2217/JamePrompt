use crate::db::Database;
use crate::models::{Prompt, PromptId, PromptQuery};
use std::fmt;

#[derive(Debug)]
pub enum PromptRepositoryError {
    Storage(rusqlite::Error),
    PromptNotFound(PromptId),
}

impl From<rusqlite::Error> for PromptRepositoryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

impl fmt::Display for PromptRepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(f, "prompt storage error: {}", error),
            Self::PromptNotFound(id) => write!(f, "prompt not found: {}", id),
        }
    }
}

impl std::error::Error for PromptRepositoryError {}

pub struct PromptRepository {
    db: Database,
}

impl PromptRepository {
    pub fn in_memory() -> Result<Self, PromptRepositoryError> {
        Ok(Self {
            db: Database::in_memory()?,
        })
    }

    pub fn list(&self, search: &str) -> Result<Vec<Prompt>, PromptRepositoryError> {
        Ok(self.db.get_all(search)?)
    }

    pub fn search(&self, term: &str) -> Result<Vec<Prompt>, PromptRepositoryError> {
        self.list(term)
    }

    pub fn query(&self, query: &PromptQuery) -> Result<Vec<Prompt>, PromptRepositoryError> {
        Ok(self.db.query(query)?)
    }

    pub fn create(&self, prompt: &Prompt) -> Result<PromptId, PromptRepositoryError> {
        Ok(self.db.insert(prompt)?)
    }

    pub fn find_by_id(&self, id: &str) -> Result<Prompt, PromptRepositoryError> {
        self.db
            .get_by_id(id)?
            .ok_or_else(|| PromptRepositoryError::PromptNotFound(id.to_string()))
    }

    pub fn update(&self, prompt: &Prompt) -> Result<(), PromptRepositoryError> {
        let _ = self.find_by_id(&prompt.id)?;
        Ok(self.db.update(prompt)?)
    }

    pub fn delete(&self, id: &str) -> Result<(), PromptRepositoryError> {
        let _ = self.find_by_id(id)?;
        Ok(self.db.delete(id)?)
    }

    pub fn set_favorite(&self, id: &str, favorite: bool) -> Result<(), PromptRepositoryError> {
        let _ = self.find_by_id(id)?;
        Ok(self.db.set_favorite(id, favorite)?)
    }

    pub fn record_use(&self, id: &str) -> Result<(), PromptRepositoryError> {
        let _ = self.find_by_id(id)?;
        Ok(self.db.record_use(id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_prompt(name: &str, content: &str) -> Prompt {
        Prompt {
            id: String::new(),
            name: name.to_string(),
            content: content.to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        }
    }

    #[test]
    fn repository_create_returns_generated_id() {
        let repository = PromptRepository::in_memory().expect("Repository should initialize");

        let id = repository
            .create(&sample_prompt("Alpha", "Content"))
            .expect("Create should succeed");

        assert!(!id.trim().is_empty());
        assert_eq!(repository.list("").expect("List should succeed").len(), 1);
    }

    #[test]
    fn repository_search_matches_name_and_content() {
        let repository = PromptRepository::in_memory().expect("Repository should initialize");
        repository
            .create(&sample_prompt("Rust Helper", "fn main() {}"))
            .expect("Create should succeed");
        repository
            .create(&sample_prompt("Python Helper", "print('hello')"))
            .expect("Create should succeed");

        let results = repository.search("Python").expect("Search should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Python Helper");
    }

    #[test]
    fn repository_update_preserves_id() {
        let repository = PromptRepository::in_memory().expect("Repository should initialize");
        let id = repository
            .create(&sample_prompt("Original", "Content"))
            .expect("Create should succeed");

        repository
            .update(&Prompt {
                id: id.clone(),
                name: "Updated".to_string(),
                content: "Updated content".to_string(),
                hotkey: None,
                hotkey_enabled: true,
                favorite: false,
                created_at: String::new(),
                updated_at: String::new(),
                last_used_at: None,
                use_count: 0,
            })
            .expect("Update should succeed");

        let stored = repository.find_by_id(&id).expect("Find should succeed");
        assert_eq!(stored.id, id);
        assert_eq!(stored.name, "Updated");
    }

    #[test]
    fn repository_delete_missing_id_reports_expected_result() {
        let repository = PromptRepository::in_memory().expect("Repository should initialize");

        let error = repository
            .delete("missing-id")
            .expect_err("Missing prompt should be reported");

        assert!(matches!(error, PromptRepositoryError::PromptNotFound(id) if id == "missing-id"));
    }
}
