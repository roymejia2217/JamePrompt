use crate::migrations::MigrationManager;
use crate::models::{Prompt, PromptFilter, PromptQuery, PromptSort};
use crate::perf;
use rusqlite::{params, Connection, Result};
use std::path::Path;
use uuid::Uuid;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        Self::initialize(&mut conn, true)?;
        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::initialize(&mut conn, false)?;
        Ok(Self { conn })
    }

    fn initialize(conn: &mut Connection, use_wal: bool) -> Result<()> {
        perf::measure("db.initialize", || {
            MigrationManager::initialize(conn, use_wal)
        })
    }

    /// Escape LIKE wildcard characters (% and _) in a search string
    /// so they are treated as literal characters in a LIKE pattern.
    fn escape_like(s: &str) -> String {
        let mut escaped = String::with_capacity(s.len() * 2);
        for c in s.chars() {
            match c {
                '%' => escaped.push_str("\\%"),
                '_' => escaped.push_str("\\_"),
                '\\' => escaped.push_str("\\\\"),
                _ => escaped.push(c),
            }
        }
        escaped
    }

    fn row_to_prompt(row: &rusqlite::Row<'_>) -> Result<Prompt> {
        Ok(Prompt {
            id: row.get(0)?,
            name: row.get(1)?,
            content: row.get(2)?,
            hotkey: row.get(3)?,
            hotkey_enabled: row.get(4)?,
            favorite: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            last_used_at: row.get(8)?,
            use_count: row.get(9)?,
        })
    }

    fn select_columns() -> &'static str {
        "id, name, content, hotkey, hotkey_enabled, favorite, created_at, updated_at, last_used_at, use_count"
    }

    pub fn get_all(&self, search: &str) -> Result<Vec<Prompt>> {
        perf::measure("db.get_all", || {
            let mut prompts = Vec::new();

            if search.is_empty() {
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT {} FROM prompts ORDER BY name COLLATE NOCASE",
                    Self::select_columns()
                ))?;
                let rows = stmt.query_map([], Self::row_to_prompt)?;
                for p in rows {
                    prompts.push(p?);
                }
            } else {
                let pattern = format!("%{}%", Self::escape_like(search));
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT {} FROM prompts WHERE name LIKE ?1 ESCAPE '\\' OR content LIKE ?2 ESCAPE '\\' ORDER BY name COLLATE NOCASE",
                    Self::select_columns()
                ))?;
                let rows = stmt.query_map(params![pattern, pattern], Self::row_to_prompt)?;
                for p in rows {
                    prompts.push(p?);
                }
            }

            Ok(prompts)
        })
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<Prompt>> {
        perf::measure("db.get_by_id", || {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT {} FROM prompts WHERE id = ?1",
                Self::select_columns()
            ))?;
            let mut rows = stmt.query(params![id])?;

            if let Some(row) = rows.next()? {
                return Ok(Some(Self::row_to_prompt(row)?));
            }

            Ok(None)
        })
    }

    pub fn query(&self, query: &PromptQuery) -> Result<Vec<Prompt>> {
        perf::measure("db.query", || {
            let pattern = format!("%{}%", Self::escape_like(&query.search));
            let order_by = match query.sort {
                PromptSort::NameAsc => "name COLLATE NOCASE ASC",
                PromptSort::RecentlyUsed => {
                    "last_used_at IS NULL ASC, last_used_at DESC, name COLLATE NOCASE ASC"
                }
                PromptSort::RecentlyUpdated => "updated_at DESC, name COLLATE NOCASE ASC",
                PromptSort::MostUsed => "use_count DESC, name COLLATE NOCASE ASC",
            };

            let mut sql = format!("SELECT {} FROM prompts", Self::select_columns());
            let has_search = !query.search.trim().is_empty();
            let favorite_only = matches!(query.filter, PromptFilter::Favorites);

            match (has_search, favorite_only) {
                (true, true) => {
                    sql.push_str(
                        " WHERE favorite = 1 AND (name LIKE ?1 ESCAPE '\\' OR content LIKE ?2 ESCAPE '\\')",
                    );
                }
                (true, false) => {
                    sql.push_str(" WHERE name LIKE ?1 ESCAPE '\\' OR content LIKE ?2 ESCAPE '\\'");
                }
                (false, true) => {
                    sql.push_str(" WHERE favorite = 1");
                }
                (false, false) => {}
            }

            sql.push_str(" ORDER BY ");
            sql.push_str(order_by);

            let mut stmt = self.conn.prepare(&sql)?;
            let rows = if has_search {
                stmt.query_map(params![pattern, pattern], Self::row_to_prompt)?
            } else {
                stmt.query_map([], Self::row_to_prompt)?
            };

            let mut prompts = Vec::new();
            for row in rows {
                prompts.push(row?);
            }

            Ok(prompts)
        })
    }

    pub fn insert(&self, prompt: &Prompt) -> Result<String> {
        perf::measure("db.insert", || {
            let id = if prompt.id.trim().is_empty() {
                Uuid::new_v4().to_string()
            } else {
                prompt.id.clone()
            };

            self.conn.execute(
                "INSERT INTO prompts (id, name, content, hotkey, hotkey_enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    prompt.name,
                    prompt.content,
                    prompt.hotkey,
                    prompt.hotkey_enabled
                ],
            )?;
            Ok(id)
        })
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        perf::measure("db.delete", || {
            self.conn
                .execute("DELETE FROM prompts WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn update(&self, prompt: &Prompt) -> Result<()> {
        perf::measure("db.update", || {
            self.conn.execute(
                "UPDATE prompts SET name = ?1, content = ?2, hotkey = ?3, hotkey_enabled = ?4, updated_at = datetime('now') WHERE id = ?5",
                params![prompt.name, prompt.content, prompt.hotkey, prompt.hotkey_enabled, prompt.id],
            )?;
            Ok(())
        })
    }

    pub fn set_favorite(&self, id: &str, favorite: bool) -> Result<()> {
        perf::measure("db.set_favorite", || {
            self.conn.execute(
                "UPDATE prompts SET favorite = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![favorite, id],
            )?;
            Ok(())
        })
    }

    pub fn record_use(&self, id: &str) -> Result<()> {
        perf::measure("db.record_use", || {
            self.conn.execute(
                "UPDATE prompts SET use_count = use_count + 1, last_used_at = datetime('now'), updated_at = datetime('now') WHERE id = ?1",
                params![id],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn create_test_db() -> Database {
        let dir = tempdir().expect("Failed to create temp dir");
        let db_path = dir.keep().join("test_prompts.db");
        Database::new(&db_path).expect("Failed to create test database")
    }

    #[test]
    fn test_in_memory_database_supports_crud() {
        let db = Database::in_memory().expect("Failed to create in-memory database");

        let prompt = sample_prompt("Memory", "In-memory content", Some("Ctrl+M"));
        let id = db.insert(&prompt).expect("Insert into memory DB failed");
        let all = db.get_all("").expect("Read from memory DB failed");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);

        db.delete(&id).expect("Delete from memory DB failed");
        assert!(
            db.get_all("").expect("Read after delete failed").is_empty(),
            "In-memory database should be empty after delete"
        );
    }

    #[test]
    fn test_database_new_falls_back_from_corrupt_file() {
        use std::fs;

        let dir = tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("corrupt.db");
        fs::write(&db_path, b"not a sqlite database").expect("Failed to write corrupt file");

        let result = Database::new(&db_path);
        assert!(
            result.is_err(),
            "Opening a corrupt database should fail so the caller can fall back"
        );
    }

    #[test]
    fn test_large_prompt_roundtrip() {
        let db = create_test_db();
        let large_content = "x".repeat(100_000);
        let prompt = Prompt {
            id: String::new(),
            name: "LargePrompt".to_string(),
            content: large_content.clone(),
            hotkey: Some("Ctrl+L".to_string()),
            hotkey_enabled: true,
            ..Prompt::default()
        };

        let id = db.insert(&prompt).expect("Large insert should succeed");
        let stored = db.get_all("").expect("Large read should succeed");
        let found = stored.iter().find(|p| p.id == id).expect("Prompt missing");

        assert_eq!(found.content.len(), large_content.len());
        assert_eq!(found.content, large_content);
    }

    fn sample_prompt(name: &str, content: &str, hotkey: Option<&str>) -> Prompt {
        Prompt {
            id: String::new(),
            name: name.to_string(),
            content: content.to_string(),
            hotkey: hotkey.map(|h| h.to_string()),
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        }
    }

    #[test]
    fn test_insert_and_get_all() {
        let db = create_test_db();

        let p1 = sample_prompt("Greeting", "Hello, world!", Some("Ctrl+G"));
        let p2 = sample_prompt("Farewell", "Goodbye, world!", None);

        let id1 = db.insert(&p1).expect("Insert p1 failed");
        let id2 = db.insert(&p2).expect("Insert p2 failed");

        assert!(
            Uuid::parse_str(&id1).is_ok(),
            "First insert should return a UUID"
        );
        assert!(
            Uuid::parse_str(&id2).is_ok(),
            "Second insert should return a UUID"
        );
        assert_ne!(id1, id2, "IDs should be unique");

        let all = db.get_all("").expect("get_all failed");
        assert_eq!(all.len(), 2, "Should have 2 prompts after two inserts");
    }

    #[test]
    fn test_get_all_ordered_by_name() {
        let db = create_test_db();

        db.insert(&sample_prompt("Zebra", "Z content", None))
            .unwrap();
        db.insert(&sample_prompt("Alpha", "A content", None))
            .unwrap();
        db.insert(&sample_prompt("Middle", "M content", None))
            .unwrap();

        let all = db.get_all("").expect("get_all failed");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].name, "Alpha");
        assert_eq!(all[1].name, "Middle");
        assert_eq!(all[2].name, "Zebra");
    }

    #[test]
    fn test_search_by_name() {
        let db = create_test_db();

        db.insert(&sample_prompt("Python Helper", "print('hello')", None))
            .unwrap();
        db.insert(&sample_prompt("Rust Helper", "fn main() {}", None))
            .unwrap();
        db.insert(&sample_prompt("Go Helper", "func main() {}", None))
            .unwrap();

        let results = db.get_all("Rust").expect("get_all with search failed");
        assert_eq!(results.len(), 1, "Should find 1 result for 'Rust'");
        assert_eq!(results[0].name, "Rust Helper");
    }

    #[test]
    fn test_search_by_content() {
        let db = create_test_db();

        db.insert(&sample_prompt(
            "Snippet A",
            "This uses Python language",
            None,
        ))
        .unwrap();
        db.insert(&sample_prompt("Snippet B", "This uses Rust language", None))
            .unwrap();

        let results = db
            .get_all("Python")
            .expect("get_all with content search failed");
        assert_eq!(
            results.len(),
            1,
            "Should find 1 result for 'Python' in content"
        );
        assert_eq!(results[0].name, "Snippet A");
    }

    #[test]
    fn test_search_returns_empty_for_no_match() {
        let db = create_test_db();

        db.insert(&sample_prompt("Hello", "World", None)).unwrap();

        let results = db.get_all("nonexistent").expect("get_all search failed");
        assert!(results.is_empty(), "Should return empty for no match");
    }

    #[test]
    fn test_delete_prompt() {
        let db = create_test_db();

        let id = db
            .insert(&sample_prompt("ToDelete", "Will be deleted", None))
            .unwrap();

        let all_before = db.get_all("").unwrap();
        assert_eq!(all_before.len(), 1);

        db.delete(&id).expect("Delete failed");

        let all_after = db.get_all("").unwrap();
        assert!(all_after.is_empty(), "Should be empty after delete");
    }

    #[test]
    fn test_delete_nonexistent_id_no_error() {
        let db = create_test_db();

        let result = db.delete(&Uuid::new_v4().to_string());
        assert!(result.is_ok(), "Deleting non-existent ID should not error");
    }

    #[test]
    fn test_insert_duplicate_name_fails() {
        let db = create_test_db();

        let p1 = sample_prompt("Unique", "First content", None);
        let p2 = sample_prompt("Unique", "Second content", None);

        db.insert(&p1).expect("First insert should succeed");
        let result = db.insert(&p2);
        assert!(
            result.is_err(),
            "Inserting duplicate name should fail (UNIQUE constraint)"
        );
    }

    #[test]
    fn test_prompt_fields_preserved() {
        let db = create_test_db();

        let prompt = sample_prompt("TestName", "TestContent", Some("Ctrl+Shift+T"));
        let id = db.insert(&prompt).unwrap();

        let retrieved = db.get_all("").unwrap();
        let found = retrieved
            .iter()
            .find(|p| p.id == id)
            .expect("Should find inserted prompt");

        assert_eq!(found.name, "TestName");
        assert_eq!(found.content, "TestContent");
        assert_eq!(found.hotkey, Some("Ctrl+Shift+T".to_string()));
    }

    #[test]
    fn test_prompt_with_none_hotkey() {
        let db = create_test_db();

        let prompt = sample_prompt("NoHotkey", "Content", None);
        let id = db.insert(&prompt).unwrap();

        let retrieved = db.get_all("").unwrap();
        let found = retrieved
            .iter()
            .find(|p| p.id == id)
            .expect("Should find inserted prompt");

        assert_eq!(found.hotkey, None);
    }

    #[test]
    fn test_sql_injection_in_insert_does_not_drop_table() {
        let db = create_test_db();

        let malicious_prompt = sample_prompt(
            "Injected'); DROP TABLE prompts; --",
            "Malicious content",
            None,
        );
        let inserted = db
            .insert(&malicious_prompt)
            .expect("Parameterized insert should succeed safely");

        let all = db
            .get_all("")
            .expect("Table should still exist after insert attempt");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, inserted);
        assert_eq!(all[0].name, malicious_prompt.name);
    }

    // --- SQL Injection Tests ---

    #[test]
    fn test_sql_injection_in_search_does_not_drop_table() {
        let db = create_test_db();

        db.insert(&sample_prompt("Safe", "Normal content", None))
            .unwrap();

        let malicious_search = "'; DROP TABLE prompts; --";
        let _ = db.get_all(malicious_search);

        let all = db
            .get_all("")
            .expect("Table should still exist after injection attempt");
        assert_eq!(
            all.len(),
            1,
            "Data should still be intact after injection attempt"
        );
    }

    #[test]
    fn test_sql_injection_in_search_with_percent_signs() {
        let db = create_test_db();

        db.insert(&sample_prompt("Test", "Hello World", None))
            .unwrap();

        let results = db.get_all("%");
        assert!(results.is_ok(), "Search with % should not crash");
        // With parameterized queries, searching for "%" should look for literal "%"
        // Since our data doesn't contain "%", it should return 0 results
        assert!(
            results.unwrap().is_empty(),
            "Parameterized search for literal % should return no results"
        );
    }

    #[test]
    fn test_sql_injection_with_quotes() {
        let db = create_test_db();

        db.insert(&sample_prompt("O'Brien", "Irish name", None))
            .unwrap();

        let results = db.get_all("'");
        assert!(results.is_ok(), "Search with quotes should not crash");
        // Should find "O'Brien" because it contains '
        let found = results.unwrap();
        assert_eq!(found.len(), 1, "Should find O'Brien with quote search");
    }

    #[test]
    fn test_search_with_special_characters() {
        let db = create_test_db();

        db.insert(&sample_prompt("Test", "Content with _ underscore", None))
            .unwrap();

        let results = db.get_all("_");
        assert!(results.is_ok(), "Search with underscore should not crash");
        // With parameterized queries, "_" is treated as literal underscore, not LIKE wildcard
        let found = results.unwrap();
        assert_eq!(found.len(), 1, "Should find the entry with underscore");
    }

    // --- Update Tests ---

    #[test]
    fn test_update_existing_prompt() {
        let db = create_test_db();

        let id = db
            .insert(&sample_prompt(
                "Original",
                "Original content",
                Some("Ctrl+O"),
            ))
            .unwrap();

        let updated = Prompt {
            id: id.clone(),
            name: "Updated".to_string(),
            content: "Updated content".to_string(),
            hotkey: Some("Ctrl+U".to_string()),
            hotkey_enabled: true,
            ..Prompt::default()
        };
        db.update(&updated).expect("Update should succeed");

        let all = db.get_all("").unwrap();
        let found = all
            .iter()
            .find(|p| p.id == id)
            .expect("Should find updated prompt");
        assert_eq!(found.name, "Updated");
        assert_eq!(found.content, "Updated content");
        assert_eq!(found.hotkey, Some("Ctrl+U".to_string()));
    }

    #[test]
    fn test_update_nonexistent_id_no_error() {
        let db = create_test_db();

        let prompt = Prompt {
            id: Uuid::new_v4().to_string(),
            name: "Ghost".to_string(),
            content: "Does not exist".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            ..Prompt::default()
        };
        let result = db.update(&prompt);
        assert!(result.is_ok(), "Updating non-existent ID should not error");
    }

    #[test]
    fn test_update_duplicate_name_fails() {
        let db = create_test_db();

        db.insert(&sample_prompt("First", "Content A", None))
            .unwrap();
        let id2 = db
            .insert(&sample_prompt("Second", "Content B", None))
            .unwrap();

        let updated = Prompt {
            id: id2.clone(),
            name: "First".to_string(),
            content: "Content B modified".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            ..Prompt::default()
        };
        let result = db.update(&updated);
        assert!(
            result.is_err(),
            "Updating to a duplicate name should fail (UNIQUE constraint)"
        );
    }

    #[test]
    fn test_update_preserves_id() {
        let db = create_test_db();

        let id = db
            .insert(&sample_prompt("PreserveId", "Content", None))
            .unwrap();

        let updated = Prompt {
            id: id.clone(),
            name: "PreserveIdUpdated".to_string(),
            content: "New content".to_string(),
            hotkey: Some("Ctrl+P".to_string()),
            hotkey_enabled: true,
            ..Prompt::default()
        };
        db.update(&updated).unwrap();

        let all = db.get_all("").unwrap();
        let found = all
            .iter()
            .find(|p| p.id == id)
            .expect("Should find prompt by original id");
        assert_eq!(found.id, id, "ID should remain unchanged after update");
        assert_eq!(found.name, "PreserveIdUpdated");
    }

    #[test]
    fn test_update_with_none_hotkey() {
        let db = create_test_db();

        let id = db
            .insert(&sample_prompt("WithHotkey", "Content", Some("Ctrl+W")))
            .unwrap();

        // Verify hotkey was stored
        let before = db.get_all("").unwrap();
        let found_before = before.iter().find(|p| p.id == id).unwrap();
        assert_eq!(found_before.hotkey, Some("Ctrl+W".to_string()));

        // Update to remove hotkey
        let updated = Prompt {
            id: id.clone(),
            name: "WithHotkey".to_string(),
            content: "Content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            ..Prompt::default()
        };
        db.update(&updated).unwrap();

        let after = db.get_all("").unwrap();
        let found_after = after.iter().find(|p| p.id == id).unwrap();
        assert_eq!(
            found_after.hotkey, None,
            "Hotkey should be None after update"
        );
    }

    #[test]
    fn test_insert_sets_metadata_defaults() {
        let db = create_test_db();
        let id = db
            .insert(&sample_prompt("Metadata", "Content", None))
            .expect("Insert should succeed");

        let prompt = db
            .get_by_id(&id)
            .expect("Lookup should succeed")
            .expect("Prompt should exist");

        assert!(!prompt.favorite);
        assert_eq!(prompt.use_count, 0);
        assert!(prompt.last_used_at.is_none());
        assert!(!prompt.created_at.trim().is_empty());
        assert!(!prompt.updated_at.trim().is_empty());
    }

    #[test]
    fn test_set_favorite_updates_prompt() {
        let db = create_test_db();
        let id = db
            .insert(&sample_prompt("Favorite", "Content", None))
            .expect("Insert should succeed");

        db.set_favorite(&id, true)
            .expect("Favorite update should succeed");

        let prompt = db.get_by_id(&id).unwrap().unwrap();
        assert!(prompt.favorite);
    }

    #[test]
    fn test_record_use_increments_count_and_sets_last_used() {
        let db = create_test_db();
        let id = db
            .insert(&sample_prompt("Used", "Content", None))
            .expect("Insert should succeed");

        db.record_use(&id).expect("Record use should succeed");
        db.record_use(&id)
            .expect("Second record use should succeed");

        let prompt = db.get_by_id(&id).unwrap().unwrap();
        assert_eq!(prompt.use_count, 2);
        assert!(prompt.last_used_at.is_some());
    }

    #[test]
    fn test_query_filters_favorites() {
        let db = create_test_db();
        let favorite_id = db
            .insert(&sample_prompt("Favorite", "Content", None))
            .expect("Insert should succeed");
        db.insert(&sample_prompt("Regular", "Content", None))
            .expect("Insert should succeed");
        db.set_favorite(&favorite_id, true).unwrap();

        let results = db
            .query(&PromptQuery {
                filter: PromptFilter::Favorites,
                ..PromptQuery::default()
            })
            .expect("Query should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, favorite_id);
    }

    #[test]
    fn test_query_sorts_by_most_used() {
        let db = create_test_db();
        let low_id = db
            .insert(&sample_prompt("Low", "Content", None))
            .expect("Insert should succeed");
        let high_id = db
            .insert(&sample_prompt("High", "Content", None))
            .expect("Insert should succeed");
        db.record_use(&low_id).unwrap();
        db.record_use(&high_id).unwrap();
        db.record_use(&high_id).unwrap();

        let results = db
            .query(&PromptQuery {
                sort: PromptSort::MostUsed,
                ..PromptQuery::default()
            })
            .expect("Query should succeed");

        assert_eq!(results[0].id, high_id);
        assert_eq!(results[1].id, low_id);
    }

    #[test]
    fn test_sql_injection_in_update_does_not_drop_table() {
        let db = create_test_db();

        let id = db
            .insert(&sample_prompt("Safe", "Safe content", None))
            .unwrap();

        let malicious = Prompt {
            id: id.clone(),
            name: "Updated'); DROP TABLE prompts; --".to_string(),
            content: "Updated content".to_string(),
            hotkey: Some("Ctrl+U".to_string()),
            hotkey_enabled: true,
            ..Prompt::default()
        };

        db.update(&malicious)
            .expect("Parameterized update should succeed safely");

        let all = db
            .get_all("")
            .expect("Table should still exist after update attempt");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);
        assert_eq!(all[0].name, malicious.name);
    }

    #[test]
    fn test_hotkey_enabled_default_on_insert() {
        let db = create_test_db();

        let prompt = sample_prompt("EnabledDefault", "Content", Some("Ctrl+E"));
        let id = db.insert(&prompt).unwrap();

        let retrieved = db.get_all("").unwrap();
        let found = retrieved
            .iter()
            .find(|p| p.id == id)
            .expect("Should find inserted prompt");

        assert!(
            found.hotkey_enabled,
            "hotkey_enabled should default to true on insert"
        );
    }

    #[test]
    fn test_hotkey_enabled_false_on_insert() {
        let db = create_test_db();

        let mut prompt = sample_prompt("DisabledHotkey", "Content", Some("Ctrl+D"));
        prompt.hotkey_enabled = false;
        let id = db.insert(&prompt).unwrap();

        let retrieved = db.get_all("").unwrap();
        let found = retrieved
            .iter()
            .find(|p| p.id == id)
            .expect("Should find inserted prompt");

        assert!(
            !found.hotkey_enabled,
            "hotkey_enabled should be false when explicitly set"
        );
    }

    #[test]
    fn test_migration_adds_hotkey_enabled_column() {
        use rusqlite::Connection;

        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("migration_test.db");

        // Create a database with the old schema (without hotkey_enabled)
        {
            let conn = Connection::open(&db_path).expect("Failed to open connection");
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS prompts (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     name TEXT NOT NULL UNIQUE,
                     content TEXT NOT NULL,
                     hotkey TEXT
                 );",
            )
            .expect("Failed to create table");

            // Insert a prompt with the old schema
            conn.execute(
                "INSERT INTO prompts (name, content, hotkey) VALUES (?1, ?2, ?3)",
                params!["OldPrompt", "Old content", "Ctrl+O"],
            )
            .expect("Failed to insert old prompt");
        }

        // Now open with Database::new() which should run the migration
        let db = Database::new(&db_path).expect("Failed to open database with migration");

        // Verify the old prompt now has hotkey_enabled = true (default)
        let prompts = db.get_all("").expect("Failed to get prompts");
        assert_eq!(prompts.len(), 1, "Should have 1 prompt after migration");
        assert_eq!(prompts[0].name, "OldPrompt");
        assert!(
            Uuid::parse_str(&prompts[0].id).is_ok(),
            "Migrated prompt should receive a UUID primary key"
        );
        assert!(
            prompts[0].hotkey_enabled,
            "Migrated prompt should have hotkey_enabled = true (DEFAULT 1)"
        );

        // Verify we can insert a new prompt with hotkey_enabled
        let new_prompt = sample_prompt("NewPrompt", "New content", None);
        db.insert(&new_prompt).expect("Failed to insert new prompt");

        let prompts = db.get_all("").expect("Failed to get prompts");
        assert_eq!(prompts.len(), 2, "Should have 2 prompts after insert");
    }
}
