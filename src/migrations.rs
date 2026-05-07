use crate::perf;
use rusqlite::{Connection, Result};
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: i64 = 3;

pub struct MigrationManager;

impl MigrationManager {
    pub fn initialize(conn: &mut Connection, use_wal: bool) -> Result<()> {
        perf::measure("db.migrations.initialize", || {
            if use_wal {
                conn.execute_batch("PRAGMA journal_mode=WAL;")?;
            }

            Self::ensure_schema_migrations_table(conn)?;

            if !Self::prompts_table_exists(conn)? {
                Self::apply_migration(conn, 1, |tx| Self::create_prompts_table(tx))?;
                Self::record_all_migrations_through_current(conn)?;
                return Ok(());
            }

            if !Self::prompts_table_has_column(conn, "hotkey_enabled")? {
                Self::apply_migration(conn, 1, |tx| {
                    tx.execute(
                        "ALTER TABLE prompts ADD COLUMN hotkey_enabled INTEGER NOT NULL DEFAULT 1",
                        [],
                    )?;
                    Ok(())
                })?;
            } else {
                Self::record_migration_if_missing(conn, 1)?;
            }

            if Self::prompts_table_id_is_integer(conn)? {
                Self::apply_migration(conn, 2, Self::migrate_prompts_to_uuid)?;
            } else {
                Self::record_migration_if_missing(conn, 2)?;
            }

            if !Self::prompts_table_has_column(conn, "favorite")? {
                Self::apply_migration(conn, 3, Self::add_prompt_metadata_columns)?;
            } else {
                Self::record_migration_if_missing(conn, 3)?;
            }

            Ok(())
        })
    }

    pub fn applied_versions(conn: &Connection) -> Result<Vec<i64>> {
        let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        let mut versions = Vec::new();

        for row in rows {
            versions.push(row?);
        }

        Ok(versions)
    }

    fn ensure_schema_migrations_table(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                 version INTEGER PRIMARY KEY NOT NULL,
                 applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );",
        )
    }

    fn apply_migration<F>(conn: &mut Connection, version: i64, migration: F) -> Result<()>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<()>,
    {
        perf::measure("db.migrations.apply_migration", || {
            if Self::migration_exists(conn, version)? {
                return Ok(());
            }

            let tx = conn.transaction()?;
            migration(&tx)?;
            tx.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [version],
            )?;
            tx.commit()
        })
    }

    fn record_migration_if_missing(conn: &Connection, version: i64) -> Result<()> {
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (?1)",
            [version],
        )?;
        Ok(())
    }

    fn record_all_migrations_through_current(conn: &Connection) -> Result<()> {
        for version in 1..=CURRENT_SCHEMA_VERSION {
            Self::record_migration_if_missing(conn, version)?;
        }
        Ok(())
    }

    fn migration_exists(conn: &Connection, version: i64) -> Result<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
            [version],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    fn create_prompts_table(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE prompts (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL UNIQUE,
                 content TEXT NOT NULL,
                 hotkey TEXT,
                 hotkey_enabled INTEGER NOT NULL DEFAULT 1,
                 favorite INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                 last_used_at TEXT,
                 use_count INTEGER NOT NULL DEFAULT 0
             );",
        )
    }

    pub(crate) fn prompts_table_exists(conn: &Connection) -> Result<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'prompts'",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    fn prompts_table_has_column(conn: &Connection, column: &str) -> Result<bool> {
        let mut stmt = conn.prepare("PRAGMA table_info(prompts)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;

        for row in rows {
            if row? == column {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn prompts_table_id_is_integer(conn: &Connection) -> Result<bool> {
        let mut stmt = conn.prepare("PRAGMA table_info(prompts)")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        for row in rows {
            let (name, column_type) = row?;
            if name == "id" {
                return Ok(column_type.eq_ignore_ascii_case("INTEGER"));
            }
        }

        Ok(true)
    }

    fn migrate_prompts_to_uuid(tx: &rusqlite::Transaction<'_>) -> Result<()> {
        perf::measure("db.migrations.migrate_prompts_to_uuid", || {
            tx.execute_batch(
                "CREATE TABLE prompts_new (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL UNIQUE,
                 content TEXT NOT NULL,
                 hotkey TEXT,
                 hotkey_enabled INTEGER NOT NULL DEFAULT 1,
                 favorite INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL DEFAULT '',
                 updated_at TEXT NOT NULL DEFAULT '',
                 last_used_at TEXT,
                 use_count INTEGER NOT NULL DEFAULT 0
             );",
            )?;

            {
                let mut stmt = tx.prepare(
                    "SELECT name, content, hotkey, hotkey_enabled FROM prompts ORDER BY rowid",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, bool>(3)?,
                    ))
                })?;

                for row in rows {
                    let (name, content, hotkey, hotkey_enabled) = row?;
                    let id = Uuid::new_v4().to_string();
                    tx.execute(
                        "INSERT INTO prompts_new (id, name, content, hotkey, hotkey_enabled, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))",
                        rusqlite::params![id, name, content, hotkey, hotkey_enabled],
                    )?;
                }
            }

            tx.execute_batch(
                "DROP TABLE prompts;
             ALTER TABLE prompts_new RENAME TO prompts;",
            )
        })
    }

    fn add_prompt_metadata_columns(tx: &rusqlite::Transaction<'_>) -> Result<()> {
        perf::measure("db.migrations.add_prompt_metadata_columns", || {
            tx.execute_batch(
                "ALTER TABLE prompts ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE prompts ADD COLUMN created_at TEXT NOT NULL DEFAULT '';
             ALTER TABLE prompts ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';
             ALTER TABLE prompts ADD COLUMN last_used_at TEXT;
             ALTER TABLE prompts ADD COLUMN use_count INTEGER NOT NULL DEFAULT 0;
             UPDATE prompts
             SET created_at = CASE WHEN created_at = '' THEN datetime('now') ELSE created_at END,
                 updated_at = CASE WHEN updated_at = '' THEN datetime('now') ELSE updated_at END;",
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn migrate_prompts_to_uuid_for_test(conn: &mut Connection) -> Result<()> {
        let tx = conn.transaction()?;
        let result = Self::migrate_prompts_to_uuid(&tx);
        match result {
            Ok(()) => tx.commit(),
            Err(error) => {
                let _ = tx.rollback();
                Err(error)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn prompts_table_has_column_for_test(
        conn: &Connection,
        column: &str,
    ) -> Result<bool> {
        Self::prompts_table_has_column(conn, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migration_manager_creates_schema_version_table() {
        let mut conn = Connection::open_in_memory().expect("In-memory connection should open");

        MigrationManager::initialize(&mut conn, false).expect("Migration should succeed");

        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
                [],
                |row| row.get(0),
            )
            .expect("Schema table check should succeed");

        assert_eq!(exists, 1);
    }

    #[test]
    fn migration_manager_applies_pending_migrations_in_order() {
        let mut conn = Connection::open_in_memory().expect("In-memory connection should open");

        MigrationManager::initialize(&mut conn, false).expect("Migration should succeed");

        let versions = MigrationManager::applied_versions(&conn).expect("Versions should load");
        assert_eq!(versions, (1..=CURRENT_SCHEMA_VERSION).collect::<Vec<_>>());
    }

    #[test]
    fn migration_manager_is_idempotent() {
        let mut conn = Connection::open_in_memory().expect("In-memory connection should open");

        MigrationManager::initialize(&mut conn, false).expect("First migration should succeed");
        MigrationManager::initialize(&mut conn, false).expect("Second migration should succeed");

        let versions = MigrationManager::applied_versions(&conn).expect("Versions should load");
        assert_eq!(versions, (1..=CURRENT_SCHEMA_VERSION).collect::<Vec<_>>());
    }

    #[test]
    fn migration_manager_rolls_back_failed_migration() {
        let mut conn = Connection::open_in_memory().expect("In-memory connection should open");
        conn.execute_batch(
            "CREATE TABLE prompts (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL UNIQUE,
                 content TEXT NOT NULL,
                 hotkey TEXT,
                 hotkey_enabled INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE prompts_new (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL UNIQUE,
                 content TEXT NOT NULL,
                 hotkey TEXT,
                 hotkey_enabled INTEGER NOT NULL DEFAULT 1
             );",
        )
        .expect("Conflicting test schema should be created");

        let result = MigrationManager::migrate_prompts_to_uuid_for_test(&mut conn);

        assert!(result.is_err(), "Conflicting migration should fail");
        assert!(
            MigrationManager::prompts_table_exists(&conn).expect("Table check should succeed"),
            "Original prompts table should survive failed migration"
        );
    }

    #[test]
    fn migration_manager_adds_prompt_metadata_columns() {
        let mut conn = Connection::open_in_memory().expect("In-memory connection should open");
        conn.execute_batch(
            "CREATE TABLE prompts (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL UNIQUE,
                 content TEXT NOT NULL,
                 hotkey TEXT,
                 hotkey_enabled INTEGER NOT NULL DEFAULT 1
             );",
        )
        .expect("Legacy schema should be created");

        MigrationManager::initialize(&mut conn, false).expect("Migration should succeed");

        for column in [
            "favorite",
            "created_at",
            "updated_at",
            "last_used_at",
            "use_count",
        ] {
            assert!(
                MigrationManager::prompts_table_has_column_for_test(&conn, column)
                    .expect("Column check should succeed"),
                "Expected metadata column {column}"
            );
        }
    }

    #[test]
    fn migration_manager_preserves_existing_prompts_when_adding_metadata() {
        let mut conn = Connection::open_in_memory().expect("In-memory connection should open");
        conn.execute_batch(
            "CREATE TABLE prompts (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL UNIQUE,
                 content TEXT NOT NULL,
                 hotkey TEXT,
                 hotkey_enabled INTEGER NOT NULL DEFAULT 1
             );
             INSERT INTO prompts (id, name, content, hotkey, hotkey_enabled)
             VALUES ('11111111-1111-4111-8111-111111111111', 'Existing', 'Content', NULL, 1);",
        )
        .expect("Legacy row should be created");

        MigrationManager::initialize(&mut conn, false).expect("Migration should succeed");

        let row: (String, bool, i64) = conn
            .query_row(
                "SELECT name, favorite, use_count FROM prompts WHERE id = '11111111-1111-4111-8111-111111111111'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("Existing row should survive migration");

        assert_eq!(row, ("Existing".to_string(), false, 0));
    }
}
