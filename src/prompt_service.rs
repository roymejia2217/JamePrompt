use crate::models::{Prompt, PromptId, PromptQuery};
use crate::prompt_repository::{PromptRepository, PromptRepositoryError};
use std::fmt;

#[derive(Debug, Clone)]
pub struct PromptInput {
    pub name: String,
    pub content: String,
    pub hotkey: Option<String>,
    pub hotkey_enabled: bool,
}

#[derive(Debug)]
pub enum PromptServiceError {
    NameRequired,
    ContentRequired,
    Repository(PromptRepositoryError),
}

impl fmt::Display for PromptServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameRequired => write!(f, "prompt name is required"),
            Self::ContentRequired => write!(f, "prompt content is required"),
            Self::Repository(error) => write!(f, "{}", error),
        }
    }
}

impl std::error::Error for PromptServiceError {}

pub struct PromptService {
    repository: PromptRepository,
}

impl PromptService {
    pub fn new(repository: PromptRepository) -> Self {
        Self { repository }
    }

    pub fn create_prompt(&self, input: PromptInput) -> Result<PromptId, PromptServiceError> {
        let prompt = Self::prompt_from_input(String::new(), input)?;
        self.repository
            .create(&prompt)
            .map_err(PromptServiceError::Repository)
    }

    pub fn update_prompt(&self, id: &str, input: PromptInput) -> Result<(), PromptServiceError> {
        let prompt = Self::prompt_from_input(id.to_string(), input)?;
        self.repository
            .update(&prompt)
            .map_err(PromptServiceError::Repository)
    }

    pub fn delete_prompt(&self, id: &str) -> Result<(), PromptServiceError> {
        self.repository
            .delete(id)
            .map_err(PromptServiceError::Repository)
    }

    pub fn search_prompts(&self, search: &str) -> Result<Vec<Prompt>, PromptServiceError> {
        self.repository
            .search(search)
            .map_err(PromptServiceError::Repository)
    }

    pub fn query_prompts(&self, query: &PromptQuery) -> Result<Vec<Prompt>, PromptServiceError> {
        self.repository
            .query(query)
            .map_err(PromptServiceError::Repository)
    }

    pub fn set_favorite(&self, id: &str, favorite: bool) -> Result<(), PromptServiceError> {
        self.repository
            .set_favorite(id, favorite)
            .map_err(PromptServiceError::Repository)
    }

    pub fn record_use(&self, id: &str) -> Result<(), PromptServiceError> {
        self.repository
            .record_use(id)
            .map_err(PromptServiceError::Repository)
    }

    fn prompt_from_input(id: PromptId, input: PromptInput) -> Result<Prompt, PromptServiceError> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(PromptServiceError::NameRequired);
        }

        let content = input.content.trim().to_string();
        if content.is_empty() {
            return Err(PromptServiceError::ContentRequired);
        }

        Ok(Prompt {
            id,
            name,
            content,
            hotkey: input.hotkey.and_then(|hotkey| {
                let trimmed = hotkey.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            }),
            hotkey_enabled: input.hotkey_enabled,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> PromptService {
        PromptService::new(PromptRepository::in_memory().expect("Repository should initialize"))
    }

    fn valid_input(name: &str) -> PromptInput {
        PromptInput {
            name: name.to_string(),
            content: "Prompt content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
        }
    }

    #[test]
    fn prompt_service_rejects_empty_name() {
        let service = service();
        let mut input = valid_input("Valid");
        input.name = "   ".to_string();

        let error = service
            .create_prompt(input)
            .expect_err("Empty names should be rejected");

        assert!(matches!(error, PromptServiceError::NameRequired));
    }

    #[test]
    fn prompt_service_rejects_empty_content() {
        let service = service();
        let mut input = valid_input("Valid");
        input.content = "   ".to_string();

        let error = service
            .create_prompt(input)
            .expect_err("Empty content should be rejected");

        assert!(matches!(error, PromptServiceError::ContentRequired));
    }

    #[test]
    fn prompt_service_maps_duplicate_name_to_repository_error() {
        let service = service();
        service
            .create_prompt(valid_input("Unique"))
            .expect("Initial create should succeed");

        let error = service
            .create_prompt(valid_input("Unique"))
            .expect_err("Duplicate name should be rejected");

        assert!(matches!(error, PromptServiceError::Repository(_)));
    }

    #[test]
    fn prompt_service_refreshes_after_create_update_delete() {
        let service = service();
        let id = service
            .create_prompt(valid_input("Lifecycle"))
            .expect("Create should succeed");
        assert_eq!(
            service
                .search_prompts("")
                .expect("Search should succeed")
                .len(),
            1
        );

        service
            .update_prompt(
                &id,
                PromptInput {
                    name: "Lifecycle Updated".to_string(),
                    ..valid_input("Ignored")
                },
            )
            .expect("Update should succeed");
        assert_eq!(
            service
                .search_prompts("Updated")
                .expect("Search should succeed")
                .len(),
            1
        );

        service.delete_prompt(&id).expect("Delete should succeed");
        assert!(service
            .search_prompts("")
            .expect("Search should succeed")
            .is_empty());
    }

    #[test]
    fn service_sets_favorite() {
        let service = service();
        let id = service
            .create_prompt(valid_input("Favorite"))
            .expect("Create should succeed");

        service
            .set_favorite(&id, true)
            .expect("Favorite update should succeed");

        let prompt = service.search_prompts("").unwrap().remove(0);
        assert!(prompt.favorite);
    }

    #[test]
    fn service_records_use() {
        let service = service();
        let id = service
            .create_prompt(valid_input("Used"))
            .expect("Create should succeed");

        service.record_use(&id).expect("Use should be recorded");

        let prompt = service.search_prompts("").unwrap().remove(0);
        assert_eq!(prompt.use_count, 1);
        assert!(prompt.last_used_at.is_some());
    }

    #[test]
    fn service_queries_prompts_with_filter_and_sort() {
        let service = service();
        let id = service
            .create_prompt(valid_input("Favorite"))
            .expect("Create should succeed");
        service
            .create_prompt(valid_input("Regular"))
            .expect("Create should succeed");
        service.set_favorite(&id, true).unwrap();

        let results = service
            .query_prompts(&PromptQuery {
                filter: crate::models::PromptFilter::Favorites,
                ..PromptQuery::default()
            })
            .expect("Query should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
    }
}
